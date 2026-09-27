use linguist_config::{edit::*, *};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};
struct Fixture {
    root: PathBuf,
    path: PathBuf,
    env: BTreeMap<String, String>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lab-edit-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("config.toml");
        std::fs::write(
            &path,
            "# keep original comments in backup\n[config]\nversion=2\n",
        )
        .unwrap();
        let env = BTreeMap::from([("HOME".into(), root.to_string_lossy().into_owned())]);
        Self { root, path, env }
    }
    fn edit(&self, scope: Scope, change: Change, execute: bool) -> Result<EditReceipt> {
        edit::edit(&self.path, &scope, &change, execute, &self.env)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn set_publishes_private_file_and_byte_exact_backup() {
    let f = Fixture::new();
    let original = std::fs::read(&f.path).unwrap();
    let receipt = f
        .edit(
            Scope::default(),
            Change::Set {
                key: "llm.model".into(),
                value: json!("reviewed:model"),
            },
            true,
        )
        .unwrap();
    assert!(receipt.changed && receipt.executed);
    assert_eq!(std::fs::read(receipt.backup.unwrap()).unwrap(), original);
    assert_eq!(
        ConfigFile::read(&f.path, &Registry::builtin())
            .unwrap()
            .values["llm.model"],
        "reviewed:model"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&f.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn invalid_candidate_and_preview_leave_original_unchanged() {
    let f = Fixture::new();
    let original = std::fs::read(&f.path).unwrap();
    assert!(
        f.edit(
            Scope::default(),
            Change::Set {
                key: "jobs.heartbeat_seconds".into(),
                value: json!(30)
            },
            true
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&f.path).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(&f.root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("backup"))
            .count(),
        0
    );
    let receipt = f
        .edit(
            Scope::default(),
            Change::Reset {
                key: None,
                all: true,
            },
            false,
        )
        .unwrap();
    assert!(!receipt.executed);
    assert_eq!(std::fs::read(&f.path).unwrap(), original);
}
#[test]
fn scoped_set_and_unset_preserve_other_scopes() {
    let f = Fixture::new();
    let scope = Scope {
        profile: Some("study".into()),
        purpose: None,
    };
    f.edit(
        scope.clone(),
        Change::Set {
            key: "llm.temperature".into(),
            value: json!(0.4),
        },
        true,
    )
    .unwrap();
    assert!(
        f.edit(
            scope.clone(),
            Change::Set {
                key: "jobs.prepare_workers".into(),
                value: json!(4)
            },
            true
        )
        .is_err()
    );
    let receipt = f
        .edit(
            scope.clone(),
            Change::Unset {
                key: "llm.temperature".into(),
            },
            true,
        )
        .unwrap();
    assert_eq!(receipt.effective.values["llm.temperature"], 0.0);
    let receipt = f
        .edit(
            scope,
            Change::Unset {
                key: "llm.temperature".into(),
            },
            true,
        )
        .unwrap();
    assert!(!receipt.changed);
    assert!(receipt.backup.is_none());
}
#[test]
fn storage_move_with_existing_state_is_blocked() {
    let f = Fixture::new();
    let state = f.root.join(".local/state/linguist-anki-bridge");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("journal"), "pending").unwrap();
    let error = f
        .edit(
            Scope::default(),
            Change::Set {
                key: "storage.state_dir".into(),
                value: json!(f.root.join("new-state")),
            },
            true,
        )
        .unwrap_err();
    assert!(error.starts_with("STORAGE_RELOCATION_BLOCKED"));
    assert_eq!(
        std::fs::read_to_string(state.join("journal")).unwrap(),
        "pending"
    );
}
#[test]
fn dormant_profile_constraints_are_validated() {
    let f = Fixture::new();
    std::fs::write(&f.path,"[config]\nversion=2\n[profiles.high.overrides.learning]\nexamples_min=10\ngenerated_examples_max=10\n").unwrap();
    assert!(
        f.edit(
            Scope {
                profile: Some("high".into()),
                purpose: None
            },
            Change::Set {
                key: "learning.generated_examples_max".into(),
                value: json!(5)
            },
            true
        )
        .is_err()
    );
}
#[test]
fn lock_contention_fails_without_waiting_or_replacement() {
    let f = Fixture::new();
    let lock = std::fs::File::create(f.root.join(".config.toml.lock")).unwrap();
    lock.try_lock().unwrap();
    let error = f
        .edit(
            Scope::default(),
            Change::Set {
                key: "llm.model".into(),
                value: json!("x"),
            },
            true,
        )
        .unwrap_err();
    assert!(error.starts_with("CONFIG_EDIT_CONFLICT"));
}
#[cfg(unix)]
#[test]
fn symlink_configuration_is_not_replaced() {
    let f = Fixture::new();
    let target = f.root.join("target.toml");
    std::fs::rename(&f.path, &target).unwrap();
    std::os::unix::fs::symlink(&target, &f.path).unwrap();
    assert!(
        f.edit(
            Scope::default(),
            Change::Set {
                key: "llm.model".into(),
                value: json!("x")
            },
            true
        )
        .is_err()
    );
    assert!(std::fs::symlink_metadata(&f.path).unwrap().is_symlink());
}
