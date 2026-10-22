use std::fs;
#[cfg(target_os = "linux")]
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PLAN_VERSION: u32 = 1;
const MIGRATION_VERSION: u32 = 1;
const TRANSACTION_VERSION: u32 = 1;
const STATE_DIR: &str = "/var/lib/nyet";
const MIGRATION_DIR: &str = "/var/lib/nyet/migrations";
const BUILD_DIR: &str = "/run/nyet/build";
const MODULE_PATH: &str = "/var/lib/nyet/nyet.nix";
const MODULE_PREVIOUS: &str = "/var/lib/nyet/nyet.nix.previous";
const WRAPPER_PATH: &str = "/var/lib/nyet/flake.nix";
const TRANSACTION_DIR: &str = "/var/lib/nyet/transactions";
const CONFIGURATION_PATH: &str = "/etc/nixos";
const MODULE_LIMIT: usize = 1024 * 1024;
const PLAN_LIFETIME: u64 = 600;

#[derive(Debug, Deserialize)]
struct PlanFile {
    version: u32,
    id: String,
    uid: u32,
    host: String,
    created: u64,
    expires: u64,
    module: String,
}

#[derive(Debug, Deserialize)]
struct MigrationPlanFile {
    version: u32,
    id: String,
    uid: u32,
    root: String,
    host: String,
    system: String,
    nixpkgs: String,
    created: u64,
    expires: u64,
    operations: Vec<MigrationRename>,
}

#[derive(Debug, Clone, Deserialize)]
struct MigrationRename {
    from: String,
    to: String,
    sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MigrationManifest {
    version: u32,
    id: String,
    root: String,
    host: String,
    created: u64,
    created_dirs: Vec<String>,
    operations: Vec<ManifestOperation>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ManifestOperation {
    Rename {
        from: String,
        to: String,
        sha256: String,
        mode: u32,
    },
    Create {
        path: String,
        sha256: String,
        mode: u32,
    },
}

struct ValidatedMigration {
    root: PathBuf,
    host: String,
    system: String,
    nixpkgs: String,
    operations: Vec<MigrationRename>,
}

#[derive(Debug, Serialize)]
struct Transaction {
    version: u32,
    id: String,
    kind: &'static str,
    plan_id: Option<String>,
    uid: u32,
    started: u64,
    finished: u64,
    status: &'static str,
    before: Option<String>,
    after: Option<String>,
    message: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("nyet-helper: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments
        .next()
        .ok_or_else(|| usage().to_string())?
        .into_string()
        .map_err(|_| usage().to_string())?;

    match command.as_str() {
        "apply" => {
            let id = arguments
                .next()
                .ok_or_else(usage)?
                .into_string()
                .map_err(|_| usage().to_string())?;
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            apply(&id)
        }
        "rollback" => {
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            rollback()
        }
        "migrate" => {
            let id = arguments
                .next()
                .ok_or_else(usage)?
                .into_string()
                .map_err(|_| usage().to_string())?;
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            migrate(&id)
        }
        "migration-rollback" => {
            let id = arguments
                .next()
                .ok_or_else(usage)?
                .into_string()
                .map_err(|_| usage().to_string())?;
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            rollback_migration(&id)
        }
        _ => Err(usage().to_string()),
    }
}

fn usage() -> &'static str {
    "usage: nyet-helper apply <plan-id> | nyet-helper migrate <plan-id> | nyet-helper rollback | nyet-helper migration-rollback <plan-id>"
}

fn apply(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err("invalid plan id".to_string());
    }

    let _lock = lock()?;
    let uid = caller_uid()?;
    let path = plan_directory(uid).join(format!("{id}.json"));
    let plan = read_plan(&path, id, uid)?;
    if !safe_host(&plan.host) {
        return Err("invalid host in plan".to_string());
    }
    if !Path::new(CONFIGURATION_PATH).join("flake.nix").exists() {
        return Err(format!("{CONFIGURATION_PATH} does not contain a flake"));
    }

    let started = unix_time();
    let before = generation();
    let previous_module = read_optional(MODULE_PATH)?;
    let previous_wrapper = read_optional(WRAPPER_PATH)?;
    let previous_previous = previous_module
        .as_ref()
        .map(|module| module.as_bytes())
        .map(|module| (MODULE_PREVIOUS, module));

    if let Some((previous_path, module)) = previous_previous {
        write_atomic(Path::new(previous_path), module, 0o644)?;
    }

    write_atomic(
        Path::new(WRAPPER_PATH),
        wrapper(&plan.host).as_bytes(),
        0o644,
    )?;
    if let Err(error) = write_atomic(Path::new(MODULE_PATH), plan.module.as_bytes(), 0o644) {
        restore(WRAPPER_PATH, previous_wrapper.as_deref())?;
        return Err(error);
    }

    let result = rebuild(&plan.host, uid);
    if result.is_err() {
        restore(MODULE_PATH, previous_module.as_deref())?;
        restore(WRAPPER_PATH, previous_wrapper.as_deref())?;
    }

    let after = generation();
    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "apply",
        plan_id: Some(plan.id.clone()),
        uid,
        started,
        finished: unix_time(),
        status: if result.is_ok() { "success" } else { "failure" },
        before,
        after,
        message: result.as_ref().err().cloned(),
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }

    result?;

    let _ = fs::remove_file(&path);
    Ok(())
}

fn rollback() -> Result<(), String> {
    let _lock = lock()?;
    let started = unix_time();
    let before = generation();
    let previous_module = read_optional(MODULE_PREVIOUS)?;
    let result = rebuild_rollback();
    let after = generation();

    if result.is_ok() {
        match previous_module {
            Some(module) => {
                write_atomic(Path::new(MODULE_PATH), module.as_bytes(), 0o644)?;
                let _ = fs::remove_file(MODULE_PREVIOUS);
            }
            None => {
                let _ = fs::remove_file(MODULE_PATH);
            }
        }
    }

    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "rollback",
        plan_id: None,
        uid: caller_uid()?,
        started,
        finished: unix_time(),
        status: if result.is_ok() { "success" } else { "failure" },
        before,
        after,
        message: result.as_ref().err().cloned(),
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }

    result
}

fn migrate(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err("invalid plan id".to_string());
    }

    let _lock = lock()?;
    let uid = caller_uid()?;
    let path = plan_directory(uid).join(format!("{id}.json"));
    let plan = read_migration_plan(&path, id, uid)?;
    let migration = validate_migration(&plan)?;
    let snapshot = Path::new(MIGRATION_DIR).join(id);
    if snapshot.exists() {
        return Err(format!("migration snapshot {id} already exists"));
    }
    if migration.root.join("flake.nix").exists() {
        return Err(format!(
            "{} already contains a flake",
            migration.root.display()
        ));
    }

    let started = unix_time();
    create_migration_parent()?;
    let manifest = match snapshot_migration(&migration, &snapshot, id, started) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&snapshot);
            return Err(error);
        }
    };
    if let Err(error) = apply_migration_operations(&migration, &manifest) {
        let rollback = restore_migration(&manifest, &snapshot);
        let _ = fs::remove_dir_all(&snapshot);
        return match rollback {
            Ok(()) => Err(error),
            Err(restore) => Err(format!("{error}; automatic rollback failed: {restore}")),
        };
    }

    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "migration",
        plan_id: Some(plan.id.clone()),
        uid,
        started,
        finished: unix_time(),
        status: "success",
        before: Some(snapshot.to_string_lossy().into_owned()),
        after: None,
        message: None,
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }

    let _ = fs::remove_file(&path);
    Ok(())
}

fn rollback_migration(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err("invalid plan id".to_string());
    }

    let _lock = lock()?;
    let snapshot = Path::new(MIGRATION_DIR).join(id);
    let manifest = read_migration_manifest(&snapshot, id)?;
    let root = fs::canonicalize(CONFIGURATION_PATH)
        .map_err(|error| format!("cannot access {CONFIGURATION_PATH}: {error}"))?;
    if manifest.root != root.to_string_lossy() {
        return Err("migration snapshot does not belong to this configuration root".to_string());
    }
    verify_migration_current(&manifest, &root)?;

    let started = unix_time();
    restore_migration(&manifest, &snapshot)?;
    let _ = fs::remove_dir_all(&snapshot);

    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "migration-rollback",
        plan_id: Some(id.to_string()),
        uid: caller_uid()?,
        started,
        finished: unix_time(),
        status: "success",
        before: Some(snapshot.to_string_lossy().into_owned()),
        after: None,
        message: None,
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }
    Ok(())
}

fn read_migration_plan(path: &Path, id: &str, uid: u32) -> Result<MigrationPlanFile, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("plan must be a regular file".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != uid {
            return Err("plan is not owned by the authenticated user".to_string());
        }
        if metadata.mode() & 0o077 != 0 {
            return Err("plan permissions are too broad".to_string());
        }
    }

    let bytes = fs::read(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if bytes.len() > MODULE_LIMIT {
        return Err("plan is too large".to_string());
    }
    let plan: MigrationPlanFile =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid plan: {error}"))?;
    if plan.version != MIGRATION_VERSION {
        return Err("unsupported migration plan version".to_string());
    }
    if plan.id != id {
        return Err("plan id does not match".to_string());
    }
    if plan.uid != uid {
        return Err("plan does not belong to the authenticated user".to_string());
    }
    let now = unix_time();
    if plan.created > now.saturating_add(30) || plan.expires < now {
        return Err("plan has expired".to_string());
    }
    if plan.expires < plan.created || plan.expires - plan.created > PLAN_LIFETIME {
        return Err("plan lifetime is invalid".to_string());
    }
    Ok(plan)
}

fn validate_migration(plan: &MigrationPlanFile) -> Result<ValidatedMigration, String> {
    if !safe_host(&plan.host) {
        return Err("invalid host in plan".to_string());
    }
    if !safe_system(&plan.system) {
        return Err("invalid system in plan".to_string());
    }
    if !safe_nixpkgs(&plan.nixpkgs) {
        return Err("unsupported nixpkgs input in plan".to_string());
    }

    let root = fs::canonicalize(&plan.root)
        .map_err(|error| format!("cannot access {}: {error}", plan.root))?;
    let expected = fs::canonicalize(CONFIGURATION_PATH)
        .map_err(|error| format!("cannot access {CONFIGURATION_PATH}: {error}"))?;
    if root != expected {
        return Err(format!("migration is restricted to {CONFIGURATION_PATH}"));
    }
    if !root.is_dir() {
        return Err("configuration root is not a directory".to_string());
    }

    let configuration_to = format!("hosts/{}/configuration.nix", plan.host);
    let hardware_to = format!("hosts/{}/hardware-configuration.nix", plan.host);
    let mut configuration = false;
    let mut hardware = false;
    for operation in &plan.operations {
        if !safe_relative(&operation.from) || !safe_relative(&operation.to) {
            return Err("migration contains an unsafe path".to_string());
        }
        if !safe_hash(&operation.sha256) {
            return Err("migration contains an invalid source hash".to_string());
        }
        if operation.from == "configuration.nix" && operation.to == configuration_to {
            if configuration {
                return Err("migration repeats configuration.nix".to_string());
            }
            configuration = true;
        } else if operation.from == "hardware-configuration.nix" && operation.to == hardware_to {
            if hardware {
                return Err("migration repeats hardware-configuration.nix".to_string());
            }
            hardware = true;
        } else {
            return Err(format!(
                "migration may only move configuration.nix and hardware-configuration.nix, not {}",
                operation.from
            ));
        }

        let source = root.join(&operation.from);
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("cannot inspect {}: {error}", operation.from))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("{} must be a regular file", operation.from));
        }
        if sha256_file(&source)? != operation.sha256 {
            return Err(format!(
                "{} changed after the plan was made",
                operation.from
            ));
        }
        if root.join(&operation.to).exists() {
            return Err(format!(
                "migration destination {} already exists",
                operation.to
            ));
        }
        ensure_parent_chain(&root, &operation.to)?;
    }
    if !configuration {
        return Err("migration must move configuration.nix".to_string());
    }
    if root.join("flake.nix").exists() {
        return Err(format!("{} already contains a flake", root.display()));
    }
    ensure_parent_chain(&root, "flake.nix")?;

    Ok(ValidatedMigration {
        root,
        host: plan.host.clone(),
        system: plan.system.clone(),
        nixpkgs: plan.nixpkgs.clone(),
        operations: plan.operations.clone(),
    })
}

fn ensure_parent_chain(root: &Path, relative: &str) -> Result<(), String> {
    let parts: Vec<&str> = relative.split('/').collect();
    let mut path = root.to_path_buf();
    for part in &parts[..parts.len().saturating_sub(1)] {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!("{} is not a real directory", path.display()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
        }
    }
    Ok(())
}

fn snapshot_migration(
    migration: &ValidatedMigration,
    snapshot: &Path,
    id: &str,
    created: u64,
) -> Result<MigrationManifest, String> {
    fs::create_dir(snapshot)
        .map_err(|error| format!("cannot create snapshot {}: {error}", snapshot.display()))?;
    fs::set_permissions(snapshot, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot secure {}: {error}", snapshot.display()))?;

    let mut created_dirs = Vec::new();
    for operation in &migration.operations {
        let mut path = migration.root.clone();
        let parts: Vec<&str> = operation.to.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            path.push(part);
            let relative = path
                .strip_prefix(&migration.root)
                .map_err(|_| "migration path escaped its root".to_string())?
                .to_string_lossy()
                .into_owned();
            if !path.exists() && !created_dirs.contains(&relative) {
                created_dirs.push(relative);
            }
        }
    }

    let mut operations = Vec::new();
    for operation in &migration.operations {
        let source = migration.root.join(&operation.from);
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("cannot inspect {}: {error}", operation.from))?;
        let mode = metadata.permissions().mode() & 0o7777;
        let destination = snapshot.join("files").join(&operation.from);
        let parent = destination
            .parent()
            .ok_or_else(|| "snapshot path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        fs::copy(&source, &destination)
            .map_err(|error| format!("cannot snapshot {}: {error}", operation.from))?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("cannot set snapshot permissions: {error}"))?;
        operations.push(ManifestOperation::Rename {
            from: operation.from.clone(),
            to: operation.to.clone(),
            sha256: operation.sha256.clone(),
            mode,
        });
    }

    let flake = flake_contents(&migration.host, &migration.system, &migration.nixpkgs);
    operations.push(ManifestOperation::Create {
        path: "flake.nix".to_string(),
        sha256: sha256_bytes(flake.as_bytes()),
        mode: 0o644,
    });

    let manifest = MigrationManifest {
        version: MIGRATION_VERSION,
        id: id.to_string(),
        root: migration.root.to_string_lossy().into_owned(),
        host: migration.host.clone(),
        created,
        created_dirs,
        operations,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("cannot serialize snapshot: {error}"))?;
    write_atomic(&snapshot.join("manifest.json"), &bytes, 0o600)?;
    Ok(manifest)
}

fn create_migration_parent() -> Result<(), String> {
    let directory = Path::new(MIGRATION_DIR);
    fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot secure {}: {error}", directory.display()))
}

fn apply_migration_operations(
    migration: &ValidatedMigration,
    manifest: &MigrationManifest,
) -> Result<(), String> {
    for directory in &manifest.created_dirs {
        let path = migration.root.join(directory);
        fs::create_dir(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("cannot set permissions on {}: {error}", path.display()))?;
    }

    for operation in &manifest.operations {
        match operation {
            ManifestOperation::Rename {
                from, to, sha256, ..
            } => {
                let source = migration.root.join(from);
                let destination = migration.root.join(to);
                fs::rename(&source, &destination)
                    .map_err(|error| format!("cannot move {from} to {to}: {error}"))?;
                if sha256_file(&destination)? != *sha256 {
                    return Err(format!("{to} changed while it was moved"));
                }
            }
            ManifestOperation::Create { path, sha256, .. } => {
                let flake = flake_contents(&migration.host, &migration.system, &migration.nixpkgs);
                write_atomic(&migration.root.join(path), flake.as_bytes(), 0o644)?;
                if sha256_file(&migration.root.join(path))? != *sha256 {
                    return Err(format!("{path} was not written correctly"));
                }
            }
        }
    }
    Ok(())
}

fn restore_migration(manifest: &MigrationManifest, snapshot: &Path) -> Result<(), String> {
    for operation in manifest.operations.iter().rev() {
        match operation {
            ManifestOperation::Create { path, .. } => {
                let target = Path::new(&manifest.root).join(path);
                match fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(format!("cannot remove {}: {error}", target.display()))
                    }
                }
            }
            ManifestOperation::Rename { from, to, mode, .. } => {
                let target = Path::new(&manifest.root).join(to);
                if target.exists() {
                    fs::remove_file(&target)
                        .map_err(|error| format!("cannot remove {}: {error}", target.display()))?;
                }
                let original = snapshot.join("files").join(from);
                let bytes = fs::read(&original)
                    .map_err(|error| format!("cannot read snapshot for {from}: {error}"))?;
                write_atomic(&Path::new(&manifest.root).join(from), &bytes, *mode)?;
            }
        }
    }

    for directory in manifest.created_dirs.iter().rev() {
        let path = Path::new(&manifest.root).join(directory);
        let _ = fs::remove_dir(path);
    }
    Ok(())
}

fn verify_migration_current(manifest: &MigrationManifest, root: &Path) -> Result<(), String> {
    for operation in &manifest.operations {
        match operation {
            ManifestOperation::Rename {
                from, to, sha256, ..
            } => {
                if root.join(from).exists() {
                    return Err(format!("{from} already exists; refusing to overwrite it"));
                }
                let path = root.join(to);
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|error| format!("cannot inspect {to}: {error}"))?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(format!("{to} is not a regular file"));
                }
                if sha256_file(&path)? != *sha256 {
                    return Err(format!("{to} was modified after migration"));
                }
            }
            ManifestOperation::Create { path, sha256, .. } => {
                let target = root.join(path);
                let metadata = fs::symlink_metadata(&target)
                    .map_err(|error| format!("cannot inspect {path}: {error}"))?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(format!("{path} is not a regular file"));
                }
                if sha256_file(&target)? != *sha256 {
                    return Err(format!("{path} was modified after migration"));
                }
            }
        }
    }
    Ok(())
}

fn read_migration_manifest(snapshot: &Path, id: &str) -> Result<MigrationManifest, String> {
    let path = snapshot.join("manifest.json");
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("cannot read migration snapshot {id}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("migration snapshot is not a regular file".to_string());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("cannot read migration snapshot {id}: {error}"))?;
    let manifest: MigrationManifest =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid snapshot: {error}"))?;
    if manifest.version != MIGRATION_VERSION || manifest.id != id {
        return Err("invalid migration snapshot".to_string());
    }
    Ok(manifest)
}

fn flake_contents(host: &str, system: &str, nixpkgs: &str) -> String {
    format!(
        r#"{{
  description = "NixOS configuration for {host}";

  inputs.nixpkgs.url = "{nixpkgs}";

  outputs = {{ self, nixpkgs }}: {{
    nixosConfigurations."{host}" = nixpkgs.lib.nixosSystem {{
      system = "{system}";
      modules = [ ./hosts/{host}/configuration.nix ];
    }};
  }};
}}
"#
    )
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn safe_system(system: &str) -> bool {
    !system.is_empty()
        && system.len() <= 64
        && system
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn safe_nixpkgs(url: &str) -> bool {
    if let Some(branch) = url.strip_prefix("github:NixOS/nixpkgs/") {
        return !branch.is_empty()
            && branch.len() <= 128
            && branch.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
                    })
            });
    }
    if let Some(path) = url.strip_prefix("path:/nix/") {
        return !path.is_empty()
            && path.len() <= 256
            && path.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
                    })
            });
    }
    false
}

fn safe_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn lock() -> Result<fs::File, String> {
    fs::create_dir_all(STATE_DIR).map_err(|error| format!("cannot create {STATE_DIR}: {error}"))?;
    fs::set_permissions(STATE_DIR, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot set permissions on {STATE_DIR}: {error}"))?;
    let path = Path::new(STATE_DIR).join("lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        return Err(format!(
            "cannot lock {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(file)
}

fn read_plan(path: &Path, id: &str, uid: u32) -> Result<PlanFile, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("plan must be a regular file".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != uid {
            return Err("plan is not owned by the authenticated user".to_string());
        }
        if metadata.mode() & 0o077 != 0 {
            return Err("plan permissions are too broad".to_string());
        }
    }

    let bytes = fs::read(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if bytes.len() > MODULE_LIMIT {
        return Err("plan is too large".to_string());
    }
    let plan: PlanFile =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid plan: {error}"))?;

    if plan.version != PLAN_VERSION {
        return Err("unsupported plan version".to_string());
    }
    if plan.id != id {
        return Err("plan id does not match".to_string());
    }
    if plan.uid != uid {
        return Err("plan does not belong to the authenticated user".to_string());
    }
    if plan.module.len() > MODULE_LIMIT {
        return Err("module is too large".to_string());
    }
    let now = unix_time();
    if plan.created > now.saturating_add(30) || plan.expires < now {
        return Err("plan has expired".to_string());
    }
    if plan.expires < plan.created || plan.expires - plan.created > PLAN_LIFETIME {
        return Err("plan lifetime is invalid".to_string());
    }
    validate_module(&plan.module)?;
    Ok(plan)
}

fn validate_module(text: &str) -> Result<(), String> {
    let mut lines = text.lines();
    if lines.next() != Some("{ config, lib, pkgs, ... }:") {
        return Err("module header is not allowed".to_string());
    }
    if lines.next() != Some("{") {
        return Err("module body must start with an attribute set".to_string());
    }

    let mut closed = false;
    for line in lines {
        if line == "}" {
            if closed {
                return Err("content after the module body".to_string());
            }
            closed = true;
            continue;
        }
        if closed {
            return Err("content after the module body".to_string());
        }
        if line.is_empty() || line.starts_with("  #") {
            return Err("module contains an unsupported line".to_string());
        }
        let assignment = line
            .strip_prefix("  ")
            .ok_or_else(|| "module line is not indented".to_string())?;
        let (path, value) = assignment
            .split_once(" = ")
            .ok_or_else(|| "module line is not an assignment".to_string())?;
        if !safe_path(path) {
            return Err(format!("unsafe option path {path:?}"));
        }
        let value = value
            .strip_suffix(';')
            .ok_or_else(|| "module assignment must end with ';'".to_string())?;
        let mut parser = Parser::new(value);
        parser.literal()?;
        parser.whitespace();
        if !parser.done() {
            return Err(format!("unexpected content in value for {path}"));
        }
    }

    if !closed {
        return Err("module body is not closed".to_string());
    }
    Ok(())
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            depth: 0,
        }
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("value is nested too deeply".to_string());
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn done(&self) -> bool {
        self.position == self.input.len()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.position..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(character) if character.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn expect(&mut self, character: char) -> Result<(), String> {
        if self.bump() == Some(character) {
            Ok(())
        } else {
            Err(format!("expected {character:?}"))
        }
    }

    fn literal(&mut self) -> Result<(), String> {
        self.whitespace();
        match self.peek() {
            Some('"') => self.string(),
            Some('[') => self.list(),
            Some('{') => self.attrs(),
            Some('t') => self.keyword("true"),
            Some('f') => self.keyword("false"),
            Some('n') => self.keyword("null"),
            Some('p') => self.package(),
            Some(character) if character == '-' || character.is_ascii_digit() => self.number(),
            Some(character) => Err(format!("unsupported value starting with {character:?}")),
            None => Err("missing value".to_string()),
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<(), String> {
        if self.rest().starts_with(keyword) {
            self.position += keyword.len();
            Ok(())
        } else {
            Err(format!("expected {keyword}"))
        }
    }

    fn string(&mut self) -> Result<(), String> {
        self.expect('"')?;
        loop {
            match self.bump() {
                Some('"') => return Ok(()),
                Some('\\') => match self.bump() {
                    Some('\\' | '"' | 'n' | 'r' | 't' | '$') => {}
                    Some(character) => return Err(format!("unsupported escape \\{character}")),
                    None => return Err("unterminated escape".to_string()),
                },
                Some('$') if self.peek() == Some('{') => {
                    return Err("interpolation is not allowed".to_string());
                }
                Some(character) if character.is_control() => {
                    return Err("control character in string".to_string());
                }
                Some(_) => {}
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn list(&mut self) -> Result<(), String> {
        self.enter()?;
        self.expect('[')?;
        self.whitespace();
        while self.peek() != Some(']') {
            self.literal()?;
            self.whitespace();
        }
        self.expect(']')?;
        self.leave();
        Ok(())
    }

    fn attrs(&mut self) -> Result<(), String> {
        self.enter()?;
        self.expect('{')?;
        self.whitespace();
        while self.peek() != Some('}') {
            self.attribute_name()?;
            self.whitespace();
            self.expect('=')?;
            self.literal()?;
            self.whitespace();
            self.expect(';')?;
            self.whitespace();
        }
        self.expect('}')?;
        self.leave();
        Ok(())
    }

    fn attribute_name(&mut self) -> Result<(), String> {
        self.whitespace();
        if self.peek() == Some('"') {
            return self.string();
        }
        let start = self.position;
        if !matches!(self.peek(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        {
            return Err("invalid attribute name".to_string());
        }
        while matches!(
            self.peek(),
            Some(character)
                if character.is_ascii_alphanumeric()
                    || matches!(character, '_' | '\'' | '-')
        ) {
            self.position += 1;
        }
        if self.position == start {
            Err("invalid attribute name".to_string())
        } else {
            Ok(())
        }
    }

    fn package(&mut self) -> Result<(), String> {
        if !self.rest().starts_with("pkgs.") {
            return Err("only pkgs.<attrpath> package expressions are allowed".to_string());
        }
        self.position += "pkgs.".len();
        loop {
            let start = self.position;
            if !matches!(self.peek(), Some(character) if character.is_ascii_alphabetic() || character == '_')
            {
                return Err("invalid package attribute".to_string());
            }
            while matches!(
                self.peek(),
                Some(character)
                    if character.is_ascii_alphanumeric()
                        || matches!(character, '_' | '\'' | '-')
            ) {
                self.position += 1;
            }
            if self.position == start {
                return Err("invalid package attribute".to_string());
            }
            if self.peek() != Some('.') {
                return Ok(());
            }
            self.position += 1;
        }
    }

    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some('-') {
            self.position += 1;
        }
        let digits = self.digits()?;
        if digits == 0 {
            return Err("invalid number".to_string());
        }
        if self.peek() == Some('.') {
            self.position += 1;
            if self.digits()? == 0 {
                return Err("invalid number".to_string());
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.position += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            if self.digits()? == 0 {
                return Err("invalid number".to_string());
            }
        }
        Ok(())
    }

    fn digits(&mut self) -> Result<usize, String> {
        let start = self.position;
        while matches!(self.peek(), Some(character) if character.is_ascii_digit()) {
            self.position += 1;
        }
        Ok(self.position - start)
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|segment| {
            let mut characters = segment.chars();
            matches!(characters.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
                && characters.all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        })
}

fn safe_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 64
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn safe_id(id: &str) -> bool {
    id.len() == 32
        && id
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
}

fn plan_directory(uid: u32) -> PathBuf {
    if uid == 0 {
        PathBuf::from("/run/nyet/plans/0")
    } else {
        PathBuf::from(format!("/run/user/{uid}/nyet/plans"))
    }
}

fn wrapper(host: &str) -> String {
    format!(
        "{{\n  inputs.user.url = \"path:{CONFIGURATION_PATH}\";\n  outputs = {{ self, user }}: {{\n    nixosConfigurations.\"{host}\" = user.nixosConfigurations.\"{host}\".extendModules {{\n      modules = [ ./nyet.nix ];\n    }};\n  }};\n}}\n"
    )
}

fn rebuild(host: &str, uid: u32) -> Result<(), String> {
    build_as_user(host, uid)?;
    let flake = format!("{STATE_DIR}#{host}");
    run_nh(&[
        "os",
        "switch",
        "--bypass-root-check",
        "--elevation-strategy",
        "none",
        "--no-nom",
        "--no-specialisation",
        "-H",
        host,
        &flake,
    ])
}

fn build_as_user(host: &str, uid: u32) -> Result<(), String> {
    if uid == 0 {
        return Ok(());
    }

    let account = account(uid)
        .ok_or_else(|| format!("cannot find the requesting user {uid} in /etc/passwd"))?;
    let directory = build_directory(uid, account.gid)?;
    let result = directory.join("result");
    let flake = format!("{STATE_DIR}#{host}");
    let mut command = Command::new(nh_binary()?);
    command
        .args([
            "os",
            "build",
            "--elevation-strategy",
            "none",
            "--no-nom",
            "--no-specialisation",
            "-H",
            host,
            "--out-link",
        ])
        .arg(&result)
        .arg(&flake)
        .args(["--", "--no-write-lock-file"])
        .env_clear()
        .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin")
        .env("HOME", &account.home)
        .env("USER", &account.name)
        .env("LOGNAME", &account.name)
        .current_dir(&directory);

    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    if runtime.exists() {
        command.env("XDG_RUNTIME_DIR", runtime);
    }

    let gid = account.gid;
    unsafe {
        command.pre_exec(move || {
            if libc::setgroups(0, std::ptr::null()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setgid(gid) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setuid(uid) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let status = command
        .status()
        .map_err(|error| format!("cannot run nh as user {uid}: {error}"));
    let _ = fs::remove_dir_all(&directory);
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("nh os build failed: {status}"))
    }
}

struct Account {
    name: String,
    gid: u32,
    home: PathBuf,
}

fn account(uid: u32) -> Option<Account> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    parse_account(uid, &passwd)
}

fn parse_account(uid: u32, passwd: &str) -> Option<Account> {
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 {
            continue;
        }
        if fields[2].parse::<u32>().ok() != Some(uid) {
            continue;
        }
        let gid = fields[3].parse::<u32>().ok()?;
        let home = if fields[5].is_empty() {
            PathBuf::from("/tmp")
        } else {
            PathBuf::from(fields[5])
        };
        return Some(Account {
            name: fields[0].to_string(),
            gid,
            home,
        });
    }
    None
}

fn build_directory(uid: u32, gid: u32) -> Result<PathBuf, String> {
    let root = Path::new(BUILD_DIR);
    fs::create_dir_all(root).map_err(|error| format!("cannot create {BUILD_DIR}: {error}"))?;
    if let Some(parent) = root.parent() {
        fs::set_permissions(parent, fs::Permissions::from_mode(0o711))
            .map_err(|error| format!("cannot set permissions on {}: {error}", parent.display()))?;
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o711))
        .map_err(|error| format!("cannot set permissions on {BUILD_DIR}: {error}"))?;
    let directory = root.join(random_hex(16));
    fs::create_dir(&directory)
        .map_err(|error| format!("cannot create build directory: {error}"))?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot set build directory permissions: {error}"))?;
    std::os::unix::fs::chown(&directory, Some(uid), Some(gid))
        .map_err(|error| format!("cannot hand the build directory to {uid}: {error}"))?;
    Ok(directory)
}

fn rebuild_rollback() -> Result<(), String> {
    run_nh(&[
        "os",
        "rollback",
        "--bypass-root-check",
        "--elevation-strategy",
        "none",
        "--no-specialisation",
    ])
}

fn run_nh(args: &[&str]) -> Result<(), String> {
    let binary = nh_binary()?;
    let status = Command::new(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin")
        .env("HOME", "/root")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .status()
        .map_err(|error| format!("cannot run nh: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("nh {} {} failed: {status}", args[0], args[1]))
    }
}

fn nh_binary() -> Result<PathBuf, String> {
    [
        "/run/current-system/sw/bin/nh",
        "/nix/var/nix/profiles/system/sw/bin/nh",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|candidate| candidate.exists())
    .ok_or_else(|| "nh is not installed".to_string())
}

fn generation() -> Option<String> {
    fs::read_link("/run/current-system")
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn read_optional(path: &str) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path}: {error}")),
    }
}

fn restore(path: &str, contents: Option<&str>) -> Result<(), String> {
    match contents {
        Some(contents) => write_atomic(Path::new(path), contents.as_bytes(), 0o644),
        None => {
            let _ = fs::remove_file(path);
            Ok(())
        }
    }
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "nyet".to_string()),
        std::process::id()
    ));

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .open(&temporary)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        file.write_all(contents)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync {}: {error}", temporary.display()))?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mode;
        fs::write(&temporary, contents)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    }

    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))
}

fn write_transaction(transaction: &Transaction) -> Result<(), String> {
    let directory = Path::new(TRANSACTION_DIR);
    fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot set permissions on {}: {error}", directory.display()))?;
    let path = directory.join(format!("{}.json", transaction.id));
    let bytes = serde_json::to_vec_pretty(transaction)
        .map_err(|error| format!("cannot serialize transaction: {error}"))?;
    write_atomic(&path, &bytes, 0o644)
}

fn transaction_id(now: u64) -> String {
    format!("{now}-{}", random_hex(8))
}

fn random_hex(length: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut bytes = [0u8; 16];
    let filled = fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .is_ok();
    let seed = if filled {
        u64::from_le_bytes(bytes[..8].try_into().expect("slice length"))
    } else {
        unix_time() ^ (std::process::id() as u64) << 32
    };
    let mut state = seed;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            DIGITS[(state % 16) as usize] as char
        })
        .collect()
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn caller_uid() -> Result<u32, String> {
    if let Ok(value) = std::env::var("PKEXEC_UID") {
        return value.parse().map_err(|_| "invalid PKEXEC_UID".to_string());
    }
    if effective_uid() == 0 {
        Ok(0)
    } else {
        Err("cannot determine the authenticated user".to_string())
    }
}

fn effective_uid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self")
            .map(|metadata| metadata.uid())
            .unwrap_or(u32::MAX)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

fn ensure_root() -> Result<(), String> {
    if effective_uid() == 0 {
        Ok(())
    } else {
        Err("nyet-helper must run as root".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "{ config, lib, pkgs, ... }:\n";

    fn module(body: &str) -> String {
        format!("{HEADER}{{\n{body}}}\n")
    }

    #[test]
    fn restricts_migration_inputs() {
        assert!(safe_relative("hosts/phone/configuration.nix"));
        assert!(!safe_relative("../configuration.nix"));
        assert!(!safe_relative("/etc/passwd"));
        assert!(!safe_relative("hosts//configuration.nix"));
        assert!(safe_system("x86_64-linux"));
        assert!(!safe_system("x86_64-linux; reboot"));
        assert!(safe_nixpkgs("github:NixOS/nixpkgs/nixos-unstable"));
        assert!(safe_nixpkgs(
            "path:/nix/var/nix/profiles/per-user/root/channels/nixos"
        ));
        assert!(!safe_nixpkgs("path:/etc/passwd"));
        assert!(!safe_nixpkgs("github:NixOS/nixpkgs/../../secret"));
        assert!(safe_hash(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!safe_hash("abc"));
    }

    #[test]
    fn migration_snapshot_round_trip() {
        let directory = temporary_directory("migration");
        let root = directory.join("root");
        fs::create_dir(&root).unwrap();
        let configuration = "{ ... }: { imports = [ ./hardware-configuration.nix ]; }";
        let hardware = "{ ... }: { }";
        fs::write(root.join("configuration.nix"), configuration).unwrap();
        fs::write(root.join("hardware-configuration.nix"), hardware).unwrap();

        let migration = ValidatedMigration {
            root: root.clone(),
            host: "machine".to_string(),
            system: "x86_64-linux".to_string(),
            nixpkgs: "github:NixOS/nixpkgs/nixos-unstable".to_string(),
            operations: vec![
                MigrationRename {
                    from: "configuration.nix".to_string(),
                    to: "hosts/machine/configuration.nix".to_string(),
                    sha256: sha256_bytes(configuration.as_bytes()),
                },
                MigrationRename {
                    from: "hardware-configuration.nix".to_string(),
                    to: "hosts/machine/hardware-configuration.nix".to_string(),
                    sha256: sha256_bytes(hardware.as_bytes()),
                },
            ],
        };
        let snapshot = directory.join("snapshot");
        let manifest = snapshot_migration(&migration, &snapshot, "id", 1).unwrap();
        assert_eq!(manifest.created_dirs, ["hosts", "hosts/machine"]);

        apply_migration_operations(&migration, &manifest).unwrap();
        assert!(!root.join("configuration.nix").exists());
        assert_eq!(
            fs::read_to_string(root.join("hosts/machine/configuration.nix")).unwrap(),
            configuration
        );
        assert!(root.join("flake.nix").is_file());
        verify_migration_current(&manifest, &root).unwrap();

        restore_migration(&manifest, &snapshot).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("configuration.nix")).unwrap(),
            configuration
        );
        assert_eq!(
            fs::read_to_string(root.join("hardware-configuration.nix")).unwrap(),
            hardware
        );
        assert!(!root.join("flake.nix").exists());
        assert!(!root.join("hosts").exists());

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn refuses_to_roll_back_modified_migration_files() {
        let directory = temporary_directory("migration-modified");
        let root = directory.join("root");
        fs::create_dir(&root).unwrap();
        let configuration = "{ ... }: { }";
        fs::write(root.join("configuration.nix"), configuration).unwrap();
        let migration = ValidatedMigration {
            root: root.clone(),
            host: "machine".to_string(),
            system: "x86_64-linux".to_string(),
            nixpkgs: "github:NixOS/nixpkgs/nixos-unstable".to_string(),
            operations: vec![MigrationRename {
                from: "configuration.nix".to_string(),
                to: "hosts/machine/configuration.nix".to_string(),
                sha256: sha256_bytes(configuration.as_bytes()),
            }],
        };
        let snapshot = directory.join("snapshot");
        let manifest = snapshot_migration(&migration, &snapshot, "id", 1).unwrap();
        apply_migration_operations(&migration, &manifest).unwrap();
        fs::write(
            root.join("hosts/machine/configuration.nix"),
            "{ ... }: { boot = 1; }",
        )
        .unwrap();

        assert!(verify_migration_current(&manifest, &root).is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn accepts_rendered_scalars_lists_and_attrsets() {
        for body in [
            "  services.openssh.enable = true;\n",
            "  services.xserver.videoDrivers = [ \"modesetting\" \"nvidia\" ];\n",
            "  users.users.alice = { name = \"alice\"; uid = 1000; };\n",
            "  environment.systemPackages = [ pkgs.hello pkgs.git ];\n",
            "  services.foo.ratio = -1.5e3;\n",
            "  services.foo.nothing = null;\n",
        ] {
            assert!(validate_module(&module(body)).is_ok(), "{body}");
        }
    }

    #[test]
    fn rejects_executable_nix() {
        for body in [
            "  services.x.enable = import ./x.nix;\n",
            "  services.x.enable = builtins.readFile ./secret;\n",
            "  services.x.enable = \"${builtins.readFile ./secret}\";\n",
            "  services.x = { y = 1; } // { z = 2; };\n",
        ] {
            assert!(validate_module(&module(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn rejects_unsafe_paths_and_headers() {
        assert!(validate_module("{ ... }:\n{\n  services.x.enable = true;\n}\n").is_err());
        assert!(validate_module(&module("  services.x.\"enable\" = true;\n")).is_err());
        assert!(
            validate_module(&module("  services.x.enable = pkgs.hello.override { };\n")).is_err()
        );
        assert!(validate_module(&module("  services.x.enable = 1;\n}\n")).is_err());
        assert!(validate_module(&module(&format!(
            "  services.x.value = {};\n",
            "[".repeat(100) + &"]".repeat(100)
        )))
        .is_err());
    }

    #[test]
    fn wrapper_uses_fixed_paths_and_quoted_host() {
        let wrapper = wrapper("my-host");
        assert!(wrapper.contains("path:/etc/nixos"));
        assert!(wrapper.contains("nixosConfigurations.\"my-host\""));
        assert!(wrapper.contains("modules = [ ./nyet.nix ]"));
    }

    #[test]
    fn ids_and_hosts_are_restricted() {
        assert!(safe_id("0123456789abcdef0123456789abcdef"));
        assert!(!safe_id("../../etc/passwd"));
        assert!(safe_host("my-host_1"));
        assert!(!safe_host("host.with.dot"));
    }

    #[test]
    fn parses_the_requesting_account() {
        let passwd = "root:x:0:0:root:/root:/bin/sh\nalice:x:1000:1000:Alice:/home/alice:/bin/sh\n";
        let account = parse_account(1000, passwd).unwrap();
        assert_eq!(account.name, "alice");
        assert_eq!(account.gid, 1000);
        assert_eq!(account.home, PathBuf::from("/home/alice"));
        assert!(parse_account(1001, passwd).is_none());
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nyet-helper-{}-{}-{name}",
            std::process::id(),
            unix_time()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn plan_file(
        directory: &Path,
        id: &str,
        uid: u32,
        created: u64,
        expires: u64,
        module: &str,
    ) -> PathBuf {
        let path = directory.join(format!("{id}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": PLAN_VERSION,
            "id": id,
            "uid": uid,
            "host": "machine",
            "created": created,
            "expires": expires,
            "module": module,
        }))
        .unwrap();
        write_atomic(&path, &bytes, 0o600).unwrap();
        path
    }

    #[test]
    fn reads_a_valid_plan_and_rejects_bad_metadata() {
        let directory = temporary_directory("plan");
        let uid = effective_uid();
        let id = "0123456789abcdef0123456789abcdef";
        let module = module("  services.openssh.enable = true;\n");
        let now = unix_time();
        let path = plan_file(&directory, id, uid, now, now + 600, &module);

        assert!(read_plan(&path, id, uid).is_ok());
        assert!(read_plan(&path, "fedcba9876543210fedcba9876543210", uid).is_err());

        let expired = plan_file(&directory, id, uid, now - 1000, now - 1, &module);
        assert!(read_plan(&expired, id, uid).is_err());

        let long = plan_file(&directory, id, uid, now, now + 601, &module);
        assert!(read_plan(&long, id, uid).is_err());

        let wrong_user = plan_file(&directory, id, uid + 1, now, now + 600, &module);
        assert!(read_plan(&wrong_user, id, uid).is_err());

        let invalid = plan_file(&directory, id, uid, now, now + 600, "not a module");
        assert!(read_plan(&invalid, id, uid).is_err());

        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_a_broad_plan_mode() {
        let directory = temporary_directory("plan-mode");
        let uid = effective_uid();
        let id = "0123456789abcdef0123456789abcdef";
        let module = module("  services.openssh.enable = true;\n");
        let now = unix_time();
        let path = directory.join(format!("{id}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": PLAN_VERSION,
            "id": id,
            "uid": uid,
            "host": "machine",
            "created": now,
            "expires": now + 600,
            "module": module,
        }))
        .unwrap();
        write_atomic(&path, &bytes, 0o644).unwrap();

        assert!(read_plan(&path, id, uid).is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn plan_directories_are_uid_scoped() {
        assert_eq!(plan_directory(0), PathBuf::from("/run/nyet/plans/0"));
        assert_eq!(
            plan_directory(1000),
            PathBuf::from("/run/user/1000/nyet/plans")
        );
    }

    #[test]
    fn writes_atomically_and_restores() {
        let directory = temporary_directory("atomic");
        let path = directory.join("module.nix");
        write_atomic(&path, b"one", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        write_atomic(&path, b"two", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let _ = fs::remove_dir_all(directory);
    }
}
