//! Read-only local resource diagnostics for `config validate`.
use crate::{Effective, expand_path};
use serde::Serialize;
use std::{collections::BTreeMap, fs::OpenOptions, path::Path};

#[derive(Debug, Serialize)]
pub struct ResourceCheck {
    pub key: String,
    pub status: &'static str,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ResourceReport {
    pub checks: Vec<ResourceCheck>,
    pub missing: Vec<String>,
    pub required_missing: Vec<String>,
    pub complete: bool,
}

pub fn inspect(effective: &Effective, environment: &BTreeMap<String, String>) -> ResourceReport {
    let mut checks = Vec::new();
    let mut missing = Vec::new();
    let mut required_missing = Vec::new();
    for (key, builtin, directory) in [
        (
            "llm.prompts.vocabulary",
            Some("builtin:vocabulary-v2"),
            false,
        ),
        ("llm.prompts.grammar", Some("builtin:grammar-v2"), false),
        ("llm.prompts.kanji", Some("builtin:kanji-v2"), false),
        ("kanji.schema", Some("builtin:jisho-kanji-v2"), false),
        ("dictionary.schema_path", None, false),
        ("audio.voice_resource", None, false),
        ("ocr.resource_path", None, true),
    ] {
        let Some(reference) = effective.values.get(key).and_then(|v| v.as_str()) else {
            continue;
        };
        let status = if Some(reference) == builtin {
            "builtin_declared"
        } else if reference.starts_with("builtin:") {
            "unknown_builtin"
        } else {
            match expand_path(reference, environment) {
                Ok(path) if path.is_absolute() => inspect_path(&path, directory),
                Ok(_) => "relative_path",
                Err(_) => "invalid_path_expansion",
            }
        };
        let required = required_for(key, effective);
        if !matches!(status, "builtin_declared" | "available") {
            missing.push(key.to_owned());
            if required {
                required_missing.push(key.to_owned());
            }
        }
        checks.push(ResourceCheck {
            key: key.to_owned(),
            status,
            required,
            available_bytes: None,
            required_bytes: None,
        });
    }
    for key in ["audio.executable", "browser.executable", "ocr.executable"] {
        let Some(reference) = effective.values.get(key).and_then(|v| v.as_str()) else {
            continue;
        };
        let status = inspect_executable(reference, environment);
        let required = required_for(key, effective);
        if status != "available" {
            missing.push(key.to_owned());
            if required {
                required_missing.push(key.to_owned());
            }
        }
        checks.push(ResourceCheck {
            key: key.to_owned(),
            status,
            required,
            available_bytes: None,
            required_bytes: None,
        });
    }
    let reserve_mb = effective.values["storage.free_space_reserve_mb"]
        .as_u64()
        .expect("validated reserve");
    let required_bytes = reserve_mb * 1024 * 1024;
    let (status, available_bytes) = inspect_free_space(
        effective.values["storage.state_dir"]
            .as_str()
            .expect("validated state path"),
        environment,
        required_bytes,
    );
    if status != "available" {
        missing.push("storage.free_space_reserve_mb".into());
        required_missing.push("storage.free_space_reserve_mb".into());
    }
    checks.push(ResourceCheck {
        key: "storage.free_space_reserve_mb".into(),
        status,
        required: true,
        available_bytes,
        required_bytes: Some(required_bytes),
    });
    ResourceReport {
        checks,
        missing,
        required_missing,
        // Executable versions, provider models, schemas, hashes and service capabilities
        // still need separate checks before a plan can use them.
        complete: false,
    }
}

fn inspect_free_space(
    state_path: &str,
    environment: &BTreeMap<String, String>,
    required_bytes: u64,
) -> (&'static str, Option<u64>) {
    let path = match expand_path(state_path, environment) {
        Ok(path) if path.is_absolute() => path,
        Ok(_) => return ("relative_path", None),
        Err(_) => return ("invalid_path_expansion", None),
    };
    let mut ancestor = path.as_path();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => return ("symlink", None),
            Ok(metadata) if !metadata.is_dir() => return ("wrong_file_type", None),
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = ancestor.parent() else {
                    return ("path_unavailable", None);
                };
                ancestor = parent;
            }
            Err(_) => return ("path_unavailable", None),
        }
    }
    let mut component = ancestor.parent();
    while let Some(parent) = component {
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) if metadata.file_type().is_symlink() => return ("symlink", None),
            Ok(metadata) if metadata.is_dir() => component = parent.parent(),
            _ => return ("path_unavailable", None),
        }
    }
    let Some(available_bytes) = available_space(ancestor) else {
        return ("space_unavailable", None);
    };
    if available_bytes < required_bytes {
        ("insufficient_space", Some(available_bytes))
    } else {
        ("available", Some(available_bytes))
    }
}

#[cfg(unix)]
fn available_space(path: &Path) -> Option<u64> {
    use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut data = MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), data.as_mut_ptr()) } != 0 {
        return None;
    }
    let data = unsafe { data.assume_init() };
    let bytes = u128::from(data.f_bavail) * u128::from(data.f_frsize);
    Some(bytes.min(u128::from(u64::MAX)) as u64)
}

#[cfg(not(unix))]
fn available_space(_path: &Path) -> Option<u64> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn free_space_check_uses_existing_ancestor_and_rejects_symlinks() {
        let root = std::env::temp_dir().join(format!("lab-space-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let future = root.join("future/state");
        let future = future.to_str().unwrap();
        let environment = BTreeMap::new();
        let (status, available) = inspect_free_space(future, &environment, 1);
        assert_eq!(status, "available");
        assert!(available.unwrap() > 1);
        let (status, _) = inspect_free_space(future, &environment, u64::MAX);
        assert_eq!(status, "insufficient_space");

        let real = root.join("real");
        std::fs::create_dir(&real).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        std::fs::create_dir(real.join("state")).unwrap();
        let (status, available) =
            inspect_free_space(link.join("state").to_str().unwrap(), &environment, 1);
        assert_eq!(status, "symlink");
        assert!(available.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn required_for(key: &str, effective: &Effective) -> bool {
    let selected = |name: &str, expected: &str| {
        effective.values.get(name).and_then(|v| v.as_str()) == Some(expected)
    };
    match key {
        "llm.prompts.vocabulary" | "llm.prompts.grammar" => {
            effective
                .values
                .get("llm.enabled")
                .and_then(|v| v.as_bool())
                == Some(true)
        }
        "llm.prompts.kanji" => {
            effective
                .values
                .get("llm.enabled")
                .and_then(|v| v.as_bool())
                == Some(true)
                && effective
                    .values
                    .get("kanji.enabled")
                    .and_then(|v| v.as_bool())
                    == Some(true)
        }
        "kanji.schema" => {
            effective
                .values
                .get("kanji.enabled")
                .and_then(|v| v.as_bool())
                == Some(true)
        }
        "dictionary.schema_path" => selected("dictionary.provider", "custom"),
        "audio.voice_resource" | "audio.executable" => selected("audio.provider", "piper"),
        "ocr.resource_path" => selected("ocr.engine", "paddleocr"),
        "ocr.executable" => selected("ocr.engine", "tesseract"),
        "browser.executable" => {
            effective
                .values
                .get("browser.enabled")
                .and_then(|v| v.as_bool())
                == Some(true)
        }
        _ => false,
    }
}

fn inspect_executable(reference: &str, environment: &BTreeMap<String, String>) -> &'static str {
    match resolve_executable(reference, environment) {
        Ok(_) => "available",
        Err(status) => status,
    }
}

/// Resolve a configured helper to an absolute regular executable file without
/// invoking it. Bare names search only absolute `PATH` entries; the error is the
/// same status string reported by `config validate`.
pub fn resolve_executable(
    reference: &str,
    environment: &BTreeMap<String, String>,
) -> Result<std::path::PathBuf, &'static str> {
    if reference.contains('/') || reference.starts_with('~') || reference.contains('$') {
        return match expand_path(reference, environment) {
            Ok(path) if path.is_absolute() => match inspect_executable_path(&path) {
                "available" => Ok(path),
                status => Err(status),
            },
            Ok(_) => Err("relative_path"),
            Err(_) => Err("invalid_path_expansion"),
        };
    }
    let Some(search_path) = environment.get("PATH") else {
        return Err("path_unavailable");
    };
    let mut found_invalid = None;
    for directory in std::env::split_paths(search_path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(reference);
        let status = inspect_executable_path(&candidate);
        if status == "available" {
            return Ok(candidate);
        }
        if status != "missing" {
            found_invalid = Some(status);
        }
    }
    Err(found_invalid.unwrap_or("missing"))
}

fn inspect_executable_path(path: &Path) -> &'static str {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return "missing";
    };
    if metadata.file_type().is_symlink() {
        return "symlink";
    }
    if !metadata.is_file() {
        return "wrong_file_type";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return "not_executable";
        }
    }
    "available"
}

fn inspect_path(path: &Path, directory: bool) -> &'static str {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return "missing";
    };
    if metadata.file_type().is_symlink() {
        return "symlink";
    }
    if directory {
        return if metadata.is_dir() {
            "available"
        } else {
            "wrong_file_type"
        };
    }
    if !metadata.is_file() {
        return "wrong_file_type";
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    match options.open(path) {
        Ok(file) if file.metadata().is_ok_and(|m| m.is_file()) => "available",
        _ => "unreadable",
    }
}
