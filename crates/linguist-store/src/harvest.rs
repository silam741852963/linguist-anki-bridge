//! WP-25 word harvester: candidate words met in the deck's own examples, the
//! sentences they were met in, and a scan mark per note so a rescan reads only
//! notes whose document changed. A candidate's status (`new`, `queued`,
//! `ignored`, `known`, `added`) survives every rescan. Nothing here calls
//! Anki.
use crate::{Result, Store, sql};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS harvest_candidates(
 purpose TEXT NOT NULL, base TEXT NOT NULL, reading TEXT NOT NULL, pos TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('new','queued','ignored','known','added')),
 frequency INTEGER, updated_ms INTEGER NOT NULL, PRIMARY KEY(purpose,base));
CREATE TABLE IF NOT EXISTS harvest_contexts(
 purpose TEXT NOT NULL, note TEXT NOT NULL, base TEXT NOT NULL, sentence TEXT NOT NULL,
 translation TEXT NOT NULL, provenance TEXT NOT NULL, others TEXT NOT NULL,
 PRIMARY KEY(purpose,note,base,sentence));
CREATE INDEX IF NOT EXISTS harvest_contexts_base ON harvest_contexts(purpose,base);
CREATE TABLE IF NOT EXISTS harvest_marks(
 purpose TEXT NOT NULL, note TEXT NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(purpose,note));
";

pub const STATUSES: [&str; 5] = ["new", "queued", "ignored", "known", "added"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarvestCandidate {
    pub base: String,
    pub reading: String,
    pub pos: String,
    pub status: String,
    pub frequency: Option<u8>,
}

/// One sentence a candidate was met in. `others` are the other unknown words
/// of that sentence when it was scanned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarvestContext {
    pub note: String,
    pub base: String,
    pub sentence: String,
    pub translation: String,
    pub provenance: String,
    pub others: Vec<String>,
}

impl Store {
    fn harvest_writable(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err("STORE_READ_ONLY".into())
        }
    }

    /// The digest each note had at its last scan.
    pub fn harvest_marks(&self, purpose: &str) -> Result<BTreeMap<String, String>> {
        let mut statement = self
            .connection
            .prepare("SELECT note,digest FROM harvest_marks WHERE purpose=?1")
            .map_err(sql)?;
        let rows = statement
            .query_map([purpose], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    /// Replace one note's contexts and mark it scanned at `digest`, in one
    /// transaction. A candidate row is added for each new word (status `new`)
    /// and its reading, part of speech and frequency are refreshed.
    pub fn harvest_replace_note(
        &mut self,
        purpose: &str,
        note: &str,
        digest: &str,
        contexts: &[HarvestContext],
        candidates: &[HarvestCandidate],
        now_ms: u64,
    ) -> Result<()> {
        self.harvest_writable()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        tx.execute(
            "DELETE FROM harvest_contexts WHERE purpose=?1 AND note=?2",
            params![purpose, note],
        )
        .map_err(sql)?;
        for c in contexts {
            let others = serde_json::to_string(&c.others).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT OR IGNORE INTO harvest_contexts(purpose,note,base,sentence,translation,provenance,others) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![purpose, note, c.base, c.sentence, c.translation, c.provenance, others],
            )
            .map_err(sql)?;
        }
        for c in candidates {
            tx.execute(
                "INSERT INTO harvest_candidates(purpose,base,reading,pos,status,frequency,updated_ms) VALUES(?1,?2,?3,?4,'new',?5,?6)
                 ON CONFLICT(purpose,base) DO UPDATE SET reading=CASE WHEN excluded.reading<>'' THEN excluded.reading ELSE reading END,
                 pos=excluded.pos, frequency=excluded.frequency, updated_ms=excluded.updated_ms",
                params![purpose, c.base, c.reading, c.pos, c.frequency.map(i64::from), now_ms as i64],
            )
            .map_err(sql)?;
        }
        tx.execute(
            "INSERT INTO harvest_marks(purpose,note,digest) VALUES(?1,?2,?3) ON CONFLICT(purpose,note) DO UPDATE SET digest=excluded.digest",
            params![purpose, note, digest],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Forget the scan marks (and so the contexts) of one purpose: the next
    /// scan reads every note. Statuses stay.
    pub fn harvest_reset(&mut self, purpose: &str) -> Result<()> {
        self.harvest_writable()?;
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(sql)?;
        let outcome = self
            .connection
            .execute("DELETE FROM harvest_marks WHERE purpose=?1", [purpose])
            .and_then(|_| {
                self.connection
                    .execute("DELETE FROM harvest_contexts WHERE purpose=?1", [purpose])
            });
        match outcome {
            Ok(_) => self.connection.execute_batch("COMMIT").map_err(sql),
            Err(e) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                Err(sql(e))
            }
        }
    }

    pub fn harvest_candidates(&self, purpose: &str) -> Result<Vec<HarvestCandidate>> {
        let mut statement = self
            .connection
            .prepare("SELECT base,reading,pos,status,frequency FROM harvest_candidates WHERE purpose=?1 ORDER BY base")
            .map_err(sql)?;
        let rows = statement
            .query_map([purpose], |row| {
                Ok(HarvestCandidate {
                    base: row.get(0)?,
                    reading: row.get(1)?,
                    pos: row.get(2)?,
                    status: row.get(3)?,
                    frequency: row.get::<_, Option<i64>>(4)?.map(|f| f.clamp(0, 255) as u8),
                })
            })
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn harvest_contexts(&self, purpose: &str) -> Result<Vec<HarvestContext>> {
        let mut statement = self
            .connection
            .prepare("SELECT note,base,sentence,translation,provenance,others FROM harvest_contexts WHERE purpose=?1 ORDER BY base,note,sentence")
            .map_err(sql)?;
        let rows = statement
            .query_map([purpose], |row| {
                let others: String = row.get(5)?;
                Ok(HarvestContext {
                    note: row.get(0)?,
                    base: row.get(1)?,
                    sentence: row.get(2)?,
                    translation: row.get(3)?,
                    provenance: row.get(4)?,
                    others: serde_json::from_str(&others).unwrap_or_default(),
                })
            })
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    /// Set the status of existing candidates; returns the bases changed.
    pub fn harvest_set_status(
        &mut self,
        purpose: &str,
        bases: &[String],
        status: &str,
        now_ms: u64,
    ) -> Result<Vec<String>> {
        self.harvest_writable()?;
        if !STATUSES.contains(&status) {
            return Err(format!("HARVEST_STATUS_INVALID: {status}"));
        }
        let mut changed = Vec::new();
        for base in bases {
            let n = self
                .connection
                .execute(
                    "UPDATE harvest_candidates SET status=?3, updated_ms=?4 WHERE purpose=?1 AND base=?2",
                    params![purpose, base, status, now_ms as i64],
                )
                .map_err(sql)?;
            if n > 0 {
                changed.push(base.clone());
            }
        }
        Ok(changed)
    }

    /// Forget candidates (and their contexts) that became deck words.
    pub fn harvest_forget(&mut self, purpose: &str, bases: &[String]) -> Result<usize> {
        self.harvest_writable()?;
        let mut n = 0;
        for base in bases {
            self.connection
                .execute(
                    "DELETE FROM harvest_contexts WHERE purpose=?1 AND base=?2",
                    params![purpose, base],
                )
                .map_err(sql)?;
            n += self
                .connection
                .execute(
                    "DELETE FROM harvest_candidates WHERE purpose=?1 AND base=?2 AND status='new'",
                    params![purpose, base],
                )
                .map_err(sql)?;
        }
        Ok(n)
    }

    /// Drop candidates still `new` that no context names any more.
    pub fn harvest_prune(&mut self, purpose: &str) -> Result<usize> {
        self.harvest_writable()?;
        self.connection
            .execute(
                "DELETE FROM harvest_candidates WHERE purpose=?1 AND status='new' AND base NOT IN (SELECT base FROM harvest_contexts WHERE purpose=?1)",
                [purpose],
            )
            .map_err(sql)
    }
}
