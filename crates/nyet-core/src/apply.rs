use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const PLAN_VERSION: u32 = 1;
pub const PLAN_TTL_SECONDS: u64 = 600;

pub fn plan_directory(uid: u32) -> PathBuf {
    if uid == 0 {
        PathBuf::from("/run/nyet/plans/0")
    } else {
        PathBuf::from(format!("/run/user/{uid}/nyet/plans"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanFile {
    pub version: u32,
    pub id: String,
    pub uid: u32,
    pub host: String,
    pub created: u64,
    pub expires: u64,
    pub module: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPlan {
    pub id: String,
    pub path: PathBuf,
}

pub fn stage_plan(module: &str, host: &str) -> Result<StagedPlan> {
    let uid = current_uid()?;
    let now = unix_time()?;
    let directory = plan_directory(uid);
    stage_plan_in(&directory, module, host, uid, now)
}

pub(crate) fn stage_plan_in(
    directory: &Path,
    module: &str,
    host: &str,
    uid: u32,
    now: u64,
) -> Result<StagedPlan> {
    if !apply_safe_host(host) {
        return Err(Error::Usage(format!(
            "{host:?} is not a valid host for apply; use letters, digits, '_' or '-'"
        )));
    }
    if module.len() > 1024 * 1024 {
        return Err(Error::Usage(
            "the managed module exceeds the 1 MiB apply limit".to_string(),
        ));
    }

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;

    let id = random_id()?;
    let file = PlanFile {
        version: PLAN_VERSION,
        id: id.clone(),
        uid,
        host: host.to_string(),
        created: now,
        expires: now.saturating_add(PLAN_TTL_SECONDS),
        module: module.to_string(),
    };
    let bytes = serde_json::to_vec(&file)?;
    let path = directory.join(format!("{id}.json"));
    let mut handle = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    handle.write_all(&bytes)?;
    handle.sync_all()?;
    Ok(StagedPlan { id, path })
}

pub fn current_uid() -> Result<u32> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(std::fs::metadata("/proc/self")?.uid())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::unix::fs::MetadataExt;
        if let Some(home) = std::env::var_os("HOME") {
            if let Ok(metadata) = std::fs::metadata(home) {
                return Ok(metadata.uid());
            }
        }
        Err(Error::Usage(
            "cannot determine the current user id for plan staging".to_string(),
        ))
    }
}

pub fn unix_time() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Usage(error.to_string()))?
        .as_secs())
}

pub fn apply_safe_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 64
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

pub fn apply_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.split('.').all(|segment| {
            !segment.is_empty()
                && segment.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        })
}

fn random_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nyet-apply-{}-{}-{name}",
            std::process::id(),
            unix_time().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn stages_a_private_plan_file() {
        let directory = temporary_directory("stage");
        let staged = stage_plan_in(
            &directory,
            "{ config, lib, pkgs, ... }:\n{\n}\n",
            "machine",
            1000,
            100,
        )
        .unwrap();

        let bytes = std::fs::read(&staged.path).unwrap();
        let plan: PlanFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(plan.uid, 1000);
        assert_eq!(plan.host, "machine");
        assert_eq!(plan.expires, 100 + PLAN_TTL_SECONDS);
        assert_eq!(plan.id, staged.id);

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&staged.path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_unsafe_hosts_and_paths() {
        assert!(apply_safe_host("machine"));
        assert!(apply_safe_host("my-host_2"));
        assert!(!apply_safe_host(""));
        assert!(!apply_safe_host("host.with.dot"));
        assert!(!apply_safe_host("../root"));

        assert!(apply_safe_path("services.openssh.enable"));
        assert!(apply_safe_path("boot.kernelModules"));
        assert!(!apply_safe_path("boot.kernel.sysctl.\"kernel.hostname\""));
        assert!(!apply_safe_path("services..enable"));
    }

    #[test]
    fn generates_unique_ids() {
        let first = random_id().unwrap();
        let second = random_id().unwrap();
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }
}
