//! Scoped local edits; backups and validation precede publication.
use crate::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub profile: Option<String>,
    pub purpose: Option<String>,
}
impl Scope {
    fn record(&self) -> Result<Option<String>> {
        if self.profile.is_some() && self.purpose.is_some() {
            return Err("EDIT_SCOPE_CONFLICT: select profile or purpose".into());
        }
        match (&self.profile, &self.purpose) {
            (Some(p), _) | (_, Some(p)) if !name_valid(p) => Err("INVALID_RECORD_NAME".into()),
            (Some(p), _) => Ok(Some(format!("profiles.{p}.overrides"))),
            (_, Some(p)) => Ok(Some(format!("purposes.{p}.overrides"))),
            _ => Ok(None),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Change {
    Set { key: String, value: Value },
    Unset { key: String },
    Reset { key: Option<String>, all: bool },
}
#[derive(Debug, Serialize)]
pub struct EditReceipt {
    pub version: u16,
    pub changed: bool,
    pub executed: bool,
    pub backup: Option<PathBuf>,
    pub before_digest: String,
    pub after_digest: String,
    pub removed_keys: Vec<String>,
    pub changes: Vec<SettingChange>,
    pub effective: Effective,
    pub serialization: String,
}
#[derive(Debug, Serialize)]
pub struct SettingChange {
    pub key: String,
    pub before_override: Option<Value>,
    pub after_override: Option<Value>,
    pub before_effective: Option<Value>,
    pub after_effective: Option<Value>,
    pub before_provenance: Option<String>,
    pub after_provenance: Option<String>,
}
fn bytes(path: &Path) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|_| "CONFIG_IO: unable to open regular configuration file")?;
    if !file.metadata().map_err(|_| "CONFIG_IO")?.is_file() {
        return Err("CONFIG_NOT_REGULAR_FILE".into());
    }
    let mut data = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "CONFIG_IO")?;
    if data.len() > 4 * 1024 * 1024 {
        return Err("CONFIG_TOO_LARGE".into());
    }
    Ok(data)
}
fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|_| "CONFIG_CREATE_IO".into())
}
fn sync_dir(parent: &Path) -> Result<()> {
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "CONFIG_DIRECTORY_SYNC_FAILED".into())
}
fn serialize(config: &ConfigFile) -> Result<Vec<u8>> {
    let mut table = toml::Table::new();
    // Canonical quoted dotted keys keep all exact mappings without ambiguous nesting.
    for (k, v) in &config.values {
        if !v.is_null() {
            table.insert(
                k.clone(),
                toml::Value::try_from(v).map_err(|_| "CONFIG_SERIALIZE")?,
            );
        }
    }
    toml::to_string_pretty(&table)
        .map(String::into_bytes)
        .map_err(|_| "CONFIG_SERIALIZE".into())
}
fn validate_all(
    registry: &Registry,
    file: &ConfigFile,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let mut options = ResolveOptions {
        environment: environment.clone(),
        ..Default::default()
    };
    // Durable candidates must validate even if environment overrides mask them today.
    let clean = BTreeMap::new();
    for env in [&clean, environment] {
        options.environment = env.clone();
        resolve(registry, file, &options)?;
        let profiles: Vec<_> = file
            .values
            .keys()
            .filter(|k| k.starts_with("profiles.") && k.ends_with(".overrides"))
            .map(|k| k.split('.').nth(1).unwrap().to_owned())
            .collect();
        for purpose in presets()["presets"].as_object().unwrap().keys() {
            options.purpose = Some(purpose.clone());
            options.profile = None;
            resolve(registry, file, &options)?;
            for profile in &profiles {
                options.profile = Some(profile.clone());
                resolve(registry, file, &options)?;
            }
        }
        options.purpose = None;
        options.profile = None;
        for profile in profiles {
            options.profile = Some(profile);
            resolve(registry, file, &options)?;
        }
        options.profile = None;
    }
    Ok(())
}
fn guard_state_move(
    registry: &Registry,
    before: &ConfigFile,
    after: &ConfigFile,
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let options = ResolveOptions::default();
    let old = resolve(registry, before, &options)?;
    let new = resolve(registry, after, &options)?;
    if old.values["storage.state_dir"] == new.values["storage.state_dir"] {
        return Ok(());
    }
    let path = expand_path(
        old.values["storage.state_dir"].as_str().unwrap(),
        environment,
    )?;
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(
                    "STORAGE_RELOCATION_BLOCKED: state is nonempty; explicit migration is required"
                        .into(),
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("STORAGE_RELOCATION_BLOCKED: cannot inspect existing state".into()),
    }
    Ok(())
}
/// Replace an existing valid config with the minimal version-2 file after a
/// byte-exact private backup. This never touches the selected state directory.
pub fn replace_with_minimal(
    path: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let filename = path
        .file_name()
        .ok_or("INVALID_CONFIG_PATH")?
        .to_string_lossy();
    let lock_path = parent.join(format!(".{filename}.lock"));
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(lock_path).map_err(|_| "CONFIG_LOCK_IO")?;
    if !lock.metadata().map_err(|_| "CONFIG_LOCK_IO")?.is_file() {
        return Err("CONFIG_LOCK_IO".into());
    }
    lock.try_lock()
        .map_err(|_| "CONFIG_EDIT_CONFLICT: another editor holds the configuration lock")?;

    let original = bytes(path)?;
    let registry = Registry::builtin();
    // A malformed existing file is still replaceable: its exact bytes survive
    // in the backup. Only a valid old config can bind a state path to guard.
    let before = std::str::from_utf8(&original)
        .ok()
        .and_then(|text| ConfigFile::parse(text, &registry).ok());
    let replacement = b"[config]\nversion = 2\n";
    let after = ConfigFile::parse(std::str::from_utf8(replacement).unwrap(), &registry)?;
    validate_all(&registry, &after, environment)?;
    if let Some(before) = &before {
        guard_state_move(&registry, before, &after, environment)?;
    }

    let backup = parent.join(format!(".lab-config-backup-{}.toml", uuid::Uuid::new_v4()));
    let mut backup_file = private_file(&backup)?;
    backup_file
        .write_all(&original)
        .and_then(|_| backup_file.sync_all())
        .map_err(|_| "CONFIG_BACKUP_IO")?;
    sync_dir(parent)?;
    let temp = parent.join(format!(".lab-config-init-{}.tmp", uuid::Uuid::new_v4()));
    let outcome: Result<()> = (|| {
        let mut file = private_file(&temp)?;
        file.write_all(replacement)
            .and_then(|_| file.sync_all())
            .map_err(|_| "CONFIG_WRITE_IO")?;
        if bytes(path)? != original {
            return Err("CONFIG_EDIT_CONFLICT: configuration changed after validation".into());
        }
        std::fs::rename(&temp, path).map_err(|_| "CONFIG_PUBLISH_IO")?;
        sync_dir(parent)
    })();
    let _ = std::fs::remove_file(temp);
    outcome?;
    Ok(backup)
}
/// Preview is side-effect free. Execute serializes cooperating editors with an OS lock.
/// An external editor that ignores this lock is detected by the prepublication byte check.
pub fn edit(
    path: &Path,
    scope: &Scope,
    change: &Change,
    execute: bool,
    environment: &BTreeMap<String, String>,
) -> Result<EditReceipt> {
    let registry = Registry::builtin();
    let record = scope.record()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let _lock = if execute {
        let filename = path
            .file_name()
            .ok_or("INVALID_CONFIG_PATH")?
            .to_string_lossy();
        let lock_path = parent.join(format!(".{filename}.lock"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(lock_path).map_err(|_| "CONFIG_LOCK_IO")?;
        if !file.metadata().map_err(|_| "CONFIG_LOCK_IO")?.is_file() {
            return Err("CONFIG_LOCK_IO".into());
        }
        file.try_lock()
            .map_err(|_| "CONFIG_EDIT_CONFLICT: another editor holds the configuration lock")?;
        Some(file)
    } else {
        None
    };
    let original = bytes(path)?;
    let before = ConfigFile::parse(
        std::str::from_utf8(&original).map_err(|_| "CONFIG_ENCODING")?,
        &registry,
    )?;
    let mut candidate = before.clone();
    let mut removed = Vec::new();
    let mut target = match &record {
        Some(key) => candidate
            .values
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        None => candidate
            .values
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    };
    let original_target = target.clone();
    let check_key = |key: &str| -> Result<()> {
        let e = registry.lookup(key)?;
        if key == "config.version" || key.contains('<') {
            return Err("IMMUTABLE_OR_PATTERN_SETTING".into());
        }
        if record.is_some() && e.scope != "purpose" {
            return Err(format!("INVALID_OVERRIDE_SCOPE: {key}"));
        }
        Ok(())
    };
    match change {
        Change::Set { key, value } => {
            check_key(key)?;
            registry.validate_value(key, value)?;
            if value.is_null() {
                return Err("USE_CONFIG_UNSET_FOR_NULL".into());
            }
            target.insert(key.clone(), value.clone());
        }
        Change::Unset { key } => {
            check_key(key)?;
            if target.remove(key).is_some() {
                removed.push(key.clone());
            }
        }
        Change::Reset { key, all } => {
            if *all == key.is_some() {
                return Err("RESET_SELECTION_REQUIRED: choose KEY or --all".into());
            }
            if let Some(key) = key {
                check_key(key)?;
                if target.remove(key).is_some() {
                    removed.push(key.clone());
                }
            } else {
                removed = target
                    .keys()
                    .filter(|k| k.as_str() != "config.version")
                    .cloned()
                    .collect();
                target.retain(|k, _| k == "config.version");
            }
        }
    }
    let candidate_target = target.clone();
    match &record {
        Some(key) => {
            if !target.is_empty() || candidate.values.contains_key(key) {
                candidate.values.insert(key.clone(), Value::Object(target));
            }
        }
        None => candidate.values = target.into_iter().collect(),
    }
    validate_all(&registry, &candidate, environment)?;
    guard_state_move(&registry, &before, &candidate, environment)?;
    let effective = resolve(
        &registry,
        &candidate,
        &ResolveOptions {
            profile: scope.profile.clone(),
            purpose: scope.purpose.clone(),
            environment: environment.clone(),
            flags: BTreeMap::new(),
        },
    )?;
    let before_effective = resolve(
        &registry,
        &before,
        &ResolveOptions {
            profile: scope.profile.clone(),
            purpose: scope.purpose.clone(),
            environment: environment.clone(),
            flags: BTreeMap::new(),
        },
    )
    .ok();
    let changes = original_target
        .keys()
        .chain(candidate_target.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| original_target.get(*key) != candidate_target.get(*key))
        .map(|key| SettingChange {
            key: key.clone(),
            before_override: original_target.get(key).cloned(),
            after_override: candidate_target.get(key).cloned(),
            before_effective: before_effective
                .as_ref()
                .and_then(|e| e.values.get(key).cloned()),
            after_effective: effective.values.get(key).cloned(),
            before_provenance: before_effective
                .as_ref()
                .and_then(|e| e.provenance.get(key).cloned()),
            after_provenance: effective.provenance.get(key).cloned(),
        })
        .collect();
    let changed = before.values != candidate.values;
    let output = serialize(&candidate)?;
    ConfigFile::parse(std::str::from_utf8(&output).unwrap(), &registry)?;
    let mut receipt = EditReceipt {
        version: 2,
        changed,
        executed: false,
        backup: None,
        before_digest: canonical::asset_digest(&original),
        after_digest: if changed {
            canonical::asset_digest(&output)
        } else {
            canonical::asset_digest(&original)
        },
        removed_keys: removed,
        changes,
        effective,
        serialization: "canonical TOML; original comments retained in backup".into(),
    };
    if !execute || !changed {
        return Ok(receipt);
    }
    let backup = parent.join(format!(".lab-config-backup-{}.toml", uuid::Uuid::new_v4()));
    let mut backup_file = private_file(&backup)?;
    backup_file
        .write_all(&original)
        .and_then(|_| backup_file.sync_all())
        .map_err(|_| "CONFIG_BACKUP_IO")?;
    sync_dir(parent)?;
    receipt.backup = Some(backup);
    let temp = parent.join(format!(".lab-config-edit-{}.tmp", uuid::Uuid::new_v4()));
    let outcome: Result<()> = (|| {
        let mut file = private_file(&temp)?;
        file.write_all(&output)
            .and_then(|_| file.sync_all())
            .map_err(|_| "CONFIG_WRITE_IO")?;
        if bytes(path)? != original {
            return Err("CONFIG_EDIT_CONFLICT: configuration changed after validation".into());
        }
        std::fs::rename(&temp, path).map_err(|_| "CONFIG_PUBLISH_IO")?;
        sync_dir(parent).map_err(|_|"CONFIG_PUBLISHED_SYNC_FAILED: configuration may have changed; inspect file and backup before retry".to_owned())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    outcome?;
    receipt.executed = true;
    Ok(receipt)
}
