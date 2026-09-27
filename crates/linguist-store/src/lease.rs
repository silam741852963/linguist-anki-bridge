//! Linux worker identity and durable fencing tokens. Expiry alone never proves death.
use crate::{Result, Store, sql};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;
pub(crate) const SCHEMA_SQL: &str = "CREATE TABLE leases(resource TEXT PRIMARY KEY,token TEXT NOT NULL,pid INTEGER NOT NULL CHECK(pid>0),start_ticks INTEGER NOT NULL CHECK(start_ticks>=0),boot TEXT NOT NULL,expires_ms INTEGER NOT NULL CHECK(expires_ms>=0),generation INTEGER NOT NULL CHECK(generation>0),active INTEGER NOT NULL CHECK(active IN(0,1)));";
#[derive(Clone, Debug)]
pub enum Resource {
    JobWorker(Uuid),
    CollectionWriter(Uuid),
}
impl Resource {
    fn key(&self) -> Result<String> {
        match self {
            Self::JobWorker(id) | Self::CollectionWriter(id) if id.is_nil() => {
                Err("INVALID_LEASE_RESOURCE".into())
            }
            Self::JobWorker(id) => Ok(format!("job:{id}")),
            Self::CollectionWriter(id) => Ok(format!("writer:{id}")),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProcessIdentity {
    pid: u32,
    start_ticks: i64,
    boot: Uuid,
}
#[derive(Clone, Debug, Serialize)]
pub struct LeaseToken {
    resource: String,
    token: Uuid,
    owner: ProcessIdentity,
    generation: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Absent,
    Unknown,
}
impl ProcessIdentity {
    pub fn current() -> Result<Self> {
        let boot = boot()?;
        let pid = std::process::id();
        let start_ticks = start_ticks(pid).map_err(|_| "PROCESS_IDENTITY_UNAVAILABLE")?;
        Ok(Self {
            pid,
            start_ticks,
            boot,
        })
    }
    pub fn liveness(&self) -> Liveness {
        match boot() {
            Ok(boot) if boot != self.boot => return Liveness::Absent,
            Err(_) => return Liveness::Unknown,
            _ => {}
        }
        match start_ticks(self.pid) {
            Ok(start) if start == self.start_ticks => Liveness::Alive,
            Ok(_) => Liveness::Absent,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Liveness::Absent,
            Err(_) => Liveness::Unknown,
        }
    }
}
#[cfg(target_os = "linux")]
fn boot() -> Result<Uuid> {
    let data = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| "BOOT_ID_UNAVAILABLE")?;
    Uuid::parse_str(data.trim()).map_err(|_| "BOOT_ID_UNAVAILABLE".into())
}
#[cfg(not(target_os = "linux"))]
fn boot() -> Result<Uuid> {
    Err("LEASE_PLATFORM_UNAVAILABLE".into())
}
#[cfg(target_os = "linux")]
fn start_ticks(pid: u32) -> std::io::Result<i64> {
    let data = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    data.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(19))
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process stat"))
}
#[cfg(not(target_os = "linux"))]
fn start_ticks(_: u32) -> std::io::Result<i64> {
    Err(std::io::ErrorKind::Unsupported.into())
}
#[cfg(target_os = "linux")]
fn clock_ms() -> Result<i64> {
    let data = std::fs::read_to_string("/proc/uptime").map_err(|_| "LEASE_CLOCK_UNAVAILABLE")?;
    let seconds = data
        .split_whitespace()
        .next()
        .ok_or("LEASE_CLOCK_UNAVAILABLE")?
        .parse::<f64>()
        .map_err(|_| "LEASE_CLOCK_UNAVAILABLE")?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("LEASE_CLOCK_UNAVAILABLE".into());
    }
    Ok((seconds * 1000.0) as i64)
}
#[cfg(not(target_os = "linux"))]
fn clock_ms() -> Result<i64> {
    Err("LEASE_PLATFORM_UNAVAILABLE".into())
}
fn expiry(seconds: u64) -> Result<i64> {
    if !(1..=600).contains(&seconds) {
        return Err("INVALID_LEASE_DURATION".into());
    }
    clock_ms()?
        .checked_add((seconds * 1000) as i64)
        .ok_or("LEASE_CLOCK_EXHAUSTED".into())
}
pub(crate) fn validate_job_worker_token(
    connection: &rusqlite::Connection,
    token: &LeaseToken,
    job: Uuid,
) -> Result<()> {
    if token.resource != Resource::JobWorker(job).key()? {
        return Err("LEASE_RESOURCE_CONFLICT".into());
    }
    validate_token(connection, token)
}
fn validate_token(connection: &rusqlite::Connection, token: &LeaseToken) -> Result<()> {
    if ProcessIdentity::current()? != token.owner {
        return Err("LEASE_OWNER_CONFLICT".into());
    }
    let deadline: Option<i64> = connection.query_row(
        "SELECT expires_ms FROM leases WHERE resource=?1 AND token=?2 AND generation=?3 AND active=1",
        params![token.resource, token.token.to_string(), token.generation], |row| row.get(0),
    ).optional().map_err(sql)?;
    if deadline.is_some_and(|time| time > clock_ms().unwrap_or(i64::MAX)) {
        Ok(())
    } else {
        Err("LEASE_STALE_OR_EXPIRED".into())
    }
}
impl Store {
    pub(crate) fn validate_job_worker_lease(&self, token: &LeaseToken, job: Uuid) -> Result<()> {
        validate_job_worker_token(&self.connection, token, job)
    }
    /// Nested helpers borrow this token; acquiring the same resource again is contention.
    pub fn acquire_lease(&mut self, resource: &Resource, lease_seconds: u64) -> Result<LeaseToken> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        let key = resource.key()?;
        let owner = ProcessIdentity::current()?;
        let expires = expiry(lease_seconds)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let previous:Option<(u32,i64,String,i64,i64,bool)>=tx.query_row("SELECT pid,start_ticks,boot,expires_ms,generation,active FROM leases WHERE resource=?1",[&key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(sql)?;
        let generation =
            if let Some((pid, start_ticks, boot, deadline, generation, active)) = previous {
                let old = ProcessIdentity {
                    pid,
                    start_ticks,
                    boot: Uuid::parse_str(&boot).map_err(|_| "LEASE_CORRUPT")?,
                };
                if active
                    && (!(old.boot != owner.boot || deadline <= clock_ms()?)
                        || old.liveness() != Liveness::Absent)
                {
                    return Err("LEASE_HELD_OR_OWNER_UNVERIFIED".into());
                }
                generation
                    .checked_add(1)
                    .ok_or("LEASE_GENERATION_EXHAUSTED")?
            } else {
                1
            };
        let token = LeaseToken {
            resource: key,
            token: Uuid::new_v4(),
            owner,
            generation,
        };
        tx.execute("INSERT INTO leases(resource,token,pid,start_ticks,boot,expires_ms,generation,active) VALUES(?1,?2,?3,?4,?5,?6,?7,1) ON CONFLICT(resource) DO UPDATE SET token=excluded.token,pid=excluded.pid,start_ticks=excluded.start_ticks,boot=excluded.boot,expires_ms=excluded.expires_ms,generation=excluded.generation,active=1",params![token.resource,token.token.to_string(),token.owner.pid,token.owner.start_ticks,token.owner.boot.to_string(),expires,token.generation]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(token)
    }
    pub fn validate_lease(&self, token: &LeaseToken) -> Result<()> {
        validate_token(&self.connection, token)
    }
    pub fn renew_lease(&mut self, token: &LeaseToken, seconds: u64) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if ProcessIdentity::current()? != token.owner {
            return Err("LEASE_OWNER_CONFLICT".into());
        }
        let expires = expiry(seconds)?;
        let changed=self.connection.execute("UPDATE leases SET expires_ms=?4 WHERE resource=?1 AND token=?2 AND generation=?3 AND active=1",params![token.resource,token.token.to_string(),token.generation,expires]).map_err(sql)?;
        if changed == 1 {
            Ok(())
        } else {
            Err("LEASE_STALE_OR_RELEASED".into())
        }
    }
    pub fn release_lease(&mut self, token: &LeaseToken) -> Result<()> {
        if !self.writable {
            return Err("STORE_READ_ONLY".into());
        }
        if ProcessIdentity::current()? != token.owner {
            return Err("LEASE_OWNER_CONFLICT".into());
        }
        let changed=self.connection.execute("UPDATE leases SET active=0 WHERE resource=?1 AND token=?2 AND generation=?3 AND active=1",params![token.resource,token.token.to_string(),token.generation]).map_err(sql)?;
        if changed == 1 {
            Ok(())
        } else {
            Err("LEASE_STALE_OR_RELEASED".into())
        }
    }
}
