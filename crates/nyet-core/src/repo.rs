use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rnix::{Root, SyntaxKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::apply::{plan_directory, random_id, unix_time};
use crate::{Error, Result};

const SKIP_DIRS: &[&str] = &[".git", ".direnv", "node_modules", "result", "target"];
const CONFIGURATION: &str = "configuration.nix";
const HARDWARE: &str = "hardware-configuration.nix";
const FLAKE: &str = "flake.nix";
pub const MIGRATION_PLAN_VERSION: u32 = 1;
pub const MIGRATION_PLAN_TTL_SECONDS: u64 = 600;
pub const DEFAULT_NIXPKGS: &str = "github:NixOS/nixpkgs/nixos-unstable";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoKind {
    Flake,
    Classic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoFile {
    pub path: String,
    pub imports: Vec<String>,
    pub syntax_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub root: PathBuf,
    pub kind: RepoKind,
    pub host: Option<String>,
    pub standard: bool,
    pub files: Vec<RepoFile>,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FileOperation {
    Create { path: String, contents: String },
    Rename { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub root: PathBuf,
    pub kind: RepoKind,
    pub host: String,
    pub system: String,
    pub nixpkgs: String,
    pub operations: Vec<FileOperation>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRename {
    pub from: String,
    pub to: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlanFile {
    pub version: u32,
    pub id: String,
    pub uid: u32,
    pub root: String,
    pub host: String,
    pub system: String,
    pub nixpkgs: String,
    pub created: u64,
    pub expires: u64,
    pub operations: Vec<MigrationRename>,
}

impl MigrationPlan {
    pub fn changed_files(&self) -> Vec<String> {
        let mut files = BTreeSet::new();
        for operation in &self.operations {
            match operation {
                FileOperation::Create { path, .. } => {
                    files.insert(path.clone());
                }
                FileOperation::Rename { from, to } => {
                    files.insert(from.clone());
                    files.insert(to.clone());
                }
            }
        }
        files.into_iter().collect()
    }
}

pub fn scan(root: impl AsRef<Path>, host: Option<&str>) -> Result<Repo> {
    let root = fs::canonicalize(root.as_ref())?;
    if !root.is_dir() {
        return Err(Error::Usage(format!(
            "{} is not a configuration directory",
            root.display()
        )));
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    walk(&root, &root, &mut files, &mut warnings)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let kind = if root.join(FLAKE).is_file() {
        RepoKind::Flake
    } else {
        RepoKind::Classic
    };
    let host = host.map(ToOwned::to_owned);
    let mut missing = Vec::new();

    if kind == RepoKind::Classic && !root.join(CONFIGURATION).is_file() {
        missing.push(CONFIGURATION.to_string());
    }

    let standard = match (kind, host.as_deref()) {
        (RepoKind::Flake, Some(host)) => {
            let host_dir = root.join("hosts").join(host);
            let configuration = host_dir.join(CONFIGURATION);
            let hardware = host_dir.join(HARDWARE);
            if !configuration.is_file() {
                missing.push(format!("hosts/{host}/{CONFIGURATION}"));
            }
            if !hardware.is_file() {
                missing.push(format!("hosts/{host}/{HARDWARE}"));
            }
            configuration.is_file() && hardware.is_file()
        }
        (RepoKind::Flake, None) => {
            missing.push("host".to_string());
            false
        }
        (RepoKind::Classic, _) => false,
    };

    Ok(Repo {
        root,
        kind,
        host,
        standard,
        files,
        missing,
        warnings,
    })
}

pub fn migration_plan(repo: &Repo, host: &str, system: &str) -> Result<MigrationPlan> {
    migration_plan_with_nixpkgs(repo, host, system, DEFAULT_NIXPKGS)
}

pub fn migration_plan_with_nixpkgs(
    repo: &Repo,
    host: &str,
    system: &str,
    nixpkgs: &str,
) -> Result<MigrationPlan> {
    if repo.standard {
        return Ok(MigrationPlan {
            root: repo.root.clone(),
            kind: repo.kind,
            host: host.to_string(),
            system: system.to_string(),
            nixpkgs: nixpkgs.to_string(),
            operations: Vec::new(),
            warnings: Vec::new(),
        });
    }

    if !safe_host(host) {
        return Err(Error::Usage(format!(
            "{host:?} is not a safe host name; use letters, digits, '_' or '-'"
        )));
    }
    if !safe_system(system) {
        return Err(Error::Usage(format!(
            "{system:?} is not a safe Nix system name"
        )));
    }
    if !safe_nixpkgs(nixpkgs) {
        return Err(Error::Usage(format!(
            "{nixpkgs:?} is not a supported nixpkgs flake input"
        )));
    }

    match repo.kind {
        RepoKind::Classic => classic_plan(repo, host, system, nixpkgs),
        RepoKind::Flake => Err(Error::Usage(
            "this flake is not in the standard layout; flake migration is not implemented yet"
                .to_string(),
        )),
    }
}

fn classic_plan(repo: &Repo, host: &str, system: &str, nixpkgs: &str) -> Result<MigrationPlan> {
    let configuration = repo.root.join(CONFIGURATION);
    if !configuration.is_file() {
        return Err(Error::Usage(format!(
            "{} has no {CONFIGURATION}",
            repo.root.display()
        )));
    }

    let source = repo
        .files
        .iter()
        .find(|file| file.path == CONFIGURATION)
        .ok_or_else(|| Error::Usage(format!("{CONFIGURATION} was not indexed")))?;
    for import in &source.imports {
        let allowed = import == &format!("./{HARDWARE}") || import == HARDWARE;
        if (import.starts_with("./") || import.starts_with("../")) && !allowed {
            return Err(Error::Usage(format!(
                "{CONFIGURATION} imports {import}; migrate that import before moving the file"
            )));
        }
    }

    let host_dir = format!("hosts/{host}");
    let mut operations = vec![FileOperation::Rename {
        from: CONFIGURATION.to_string(),
        to: format!("{host_dir}/{CONFIGURATION}"),
    }];
    let mut warnings = Vec::new();
    if repo.root.join(HARDWARE).is_file() {
        operations.push(FileOperation::Rename {
            from: HARDWARE.to_string(),
            to: format!("{host_dir}/{HARDWARE}"),
        });
        if !source
            .imports
            .iter()
            .any(|import| import == &format!("./{HARDWARE}") || import == HARDWARE)
        {
            warnings.push(format!(
                "{HARDWARE} exists but {CONFIGURATION} does not import it; migration preserves that behavior"
            ));
        }
    } else {
        warnings.push(format!(
            "no {HARDWARE} found; the host will have no hardware module"
        ));
    }

    operations.push(FileOperation::Create {
        path: FLAKE.to_string(),
        contents: flake_text(host, system, nixpkgs),
    });

    Ok(MigrationPlan {
        root: repo.root.clone(),
        kind: repo.kind,
        host: host.to_string(),
        system: system.to_string(),
        nixpkgs: nixpkgs.to_string(),
        operations,
        warnings,
    })
}

fn flake_text(host: &str, system: &str, nixpkgs: &str) -> String {
    format!(
        r#"{{
  description = "NixOS configuration for {host}";

  inputs.nixpkgs.url = "{nixpkgs}";

  outputs = {{ self, nixpkgs }}: {{
    nixosConfigurations."{host}" = nixpkgs.lib.nixosSystem {{
      system = "{system}";
      modules = [ ./hosts/{host}/{CONFIGURATION} ];
    }};
  }};
}}
"#
    )
}

fn walk(
    root: &Path,
    directory: &Path,
    files: &mut Vec<RepoFile>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            warnings.push(format!(
                "skipped symlink {}",
                path.strip_prefix(root).unwrap_or(&path).display()
            ));
            continue;
        }
        if metadata.is_dir() {
            if SKIP_DIRS.iter().any(|skip| entry.file_name() == *skip) {
                continue;
            }
            walk(root, &path, files, warnings)?;
            continue;
        }
        if !metadata.is_file() || path.extension().is_none_or(|ext| ext != "nix") {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&path)?;
        let parsed = Root::parse(&text);
        let syntax_errors: Vec<String> = parsed.errors().iter().map(ToString::to_string).collect();
        let mut imports: Vec<String> = parsed
            .syntax()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::NODE_PATH)
            .map(|node| node.text().to_string())
            .filter(|text| text.starts_with("./") || text.starts_with("../"))
            .collect();
        imports.sort();
        imports.dedup();
        files.push(RepoFile {
            path: relative,
            imports,
            syntax_errors,
        });
    }
    Ok(())
}

pub fn stage_migration(plan: &MigrationPlan) -> Result<crate::apply::StagedPlan> {
    let uid = crate::current_uid()?;
    let now = unix_time()?;
    stage_migration_in(plan, &plan_directory(uid), uid, now)
}

pub(crate) fn stage_migration_in(
    plan: &MigrationPlan,
    directory: &Path,
    uid: u32,
    now: u64,
) -> Result<crate::apply::StagedPlan> {
    if plan.operations.is_empty() {
        return Err(Error::Usage(
            "the repository is already in the standard layout".to_string(),
        ));
    }
    if !safe_host(&plan.host) {
        return Err(Error::Usage(format!(
            "{:?} is not a safe host name",
            plan.host
        )));
    }
    if !safe_system(&plan.system) {
        return Err(Error::Usage(format!(
            "{:?} is not a safe Nix system name",
            plan.system
        )));
    }
    if !safe_nixpkgs(&plan.nixpkgs) {
        return Err(Error::Usage(format!(
            "{:?} is not a supported nixpkgs flake input",
            plan.nixpkgs
        )));
    }

    let root = fs::canonicalize(&plan.root)?;
    if !root.is_dir() {
        return Err(Error::Usage(format!(
            "{} is not a configuration directory",
            root.display()
        )));
    }

    let mut renames = Vec::new();
    let mut creates = Vec::new();
    for operation in &plan.operations {
        match operation {
            FileOperation::Create { path, .. } => creates.push(path.clone()),
            FileOperation::Rename { from, to } => {
                if !safe_relative(from) || !safe_relative(to) {
                    return Err(Error::Usage(format!(
                        "unsafe migration path {from:?} -> {to:?}"
                    )));
                }
                let source = root.join(from);
                let metadata = fs::symlink_metadata(&source)
                    .map_err(|error| Error::Usage(format!("cannot inspect {from}: {error}")))?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(Error::Usage(format!("{from} must be a regular file")));
                }
                if root.join(to).exists() {
                    return Err(Error::Usage(format!(
                        "migration destination {to} already exists"
                    )));
                }
                renames.push(MigrationRename {
                    from: from.clone(),
                    to: to.clone(),
                    sha256: sha256_file(&source)?,
                });
            }
        }
    }
    if creates.len() != 1 || creates[0] != FLAKE {
        return Err(Error::Usage(
            "a migration plan must create only flake.nix".to_string(),
        ));
    }
    if root.join(FLAKE).exists() {
        return Err(Error::Usage(format!(
            "{} already has a {FLAKE}",
            root.display()
        )));
    }

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;

    let id = random_id()?;
    let file = MigrationPlanFile {
        version: MIGRATION_PLAN_VERSION,
        id: id.clone(),
        uid,
        root: root.to_string_lossy().into_owned(),
        host: plan.host.clone(),
        system: plan.system.clone(),
        nixpkgs: plan.nixpkgs.clone(),
        created: now,
        expires: now.saturating_add(MIGRATION_PLAN_TTL_SECONDS),
        operations: renames,
    };
    let path = directory.join(format!("{id}.json"));
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    use std::io::Write;
    handle.write_all(&serde_json::to_vec(&file)?)?;
    handle.sync_all()?;
    Ok(crate::apply::StagedPlan { id, path })
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub fn safe_nixpkgs(url: &str) -> bool {
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

fn safe_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn safe_system(system: &str) -> bool {
    !system.is_empty()
        && system
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    #[test]
    fn scans_a_classic_configuration() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join(CONFIGURATION),
            "{ config, pkgs, ... }: { imports = [ ./hardware-configuration.nix ]; }",
        );
        write(&dir.path().join(HARDWARE), "{ ... }: { }");

        let repo = scan(dir.path(), Some("phone")).unwrap();
        assert_eq!(repo.kind, RepoKind::Classic);
        assert!(!repo.standard);
        assert_eq!(repo.files.len(), 2);
        assert_eq!(
            repo.files
                .iter()
                .find(|file| file.path == CONFIGURATION)
                .unwrap()
                .imports,
            vec!["./hardware-configuration.nix".to_string()]
        );
    }

    #[test]
    fn recognizes_a_standard_flake() {
        let dir = tempdir().unwrap();
        write(&dir.path().join(FLAKE), "{ outputs = _: { }; }");
        write(
            &dir.path().join("hosts/phone/configuration.nix"),
            "{ ... }: { }",
        );
        write(
            &dir.path().join("hosts/phone/hardware-configuration.nix"),
            "{ ... }: { }",
        );

        let repo = scan(dir.path(), Some("phone")).unwrap();
        assert_eq!(repo.kind, RepoKind::Flake);
        assert!(repo.standard);
        assert!(repo.missing.is_empty());
    }

    #[test]
    fn builds_a_classic_migration_plan() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join(CONFIGURATION),
            "{ config, pkgs, ... }: { imports = [ ./hardware-configuration.nix ]; }",
        );
        write(&dir.path().join(HARDWARE), "{ ... }: { }");

        let repo = scan(dir.path(), Some("phone")).unwrap();
        let plan = migration_plan(&repo, "phone", "x86_64-linux").unwrap();
        assert_eq!(plan.host, "phone");
        assert_eq!(plan.operations.len(), 3);
        assert!(plan.operations.iter().any(|operation| matches!(
            operation,
            FileOperation::Create { path, .. } if path == FLAKE
        )));
        assert!(plan.operations.iter().any(|operation| matches!(
            operation,
            FileOperation::Rename { from, to }
                if from == CONFIGURATION && to == "hosts/phone/configuration.nix"
        )));
    }

    #[test]
    fn refuses_relative_imports_that_migration_cannot_move() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join(CONFIGURATION),
            "{ ... }: { imports = [ ./modules/desktop.nix ]; }",
        );
        write(&dir.path().join("modules/desktop.nix"), "{ ... }: { }");

        let repo = scan(dir.path(), Some("phone")).unwrap();
        let error = migration_plan(&repo, "phone", "x86_64-linux").unwrap_err();
        assert!(error.to_string().contains("./modules/desktop.nix"));
    }

    #[test]
    fn stages_a_migration_plan_with_source_hashes() {
        let dir = tempdir().unwrap();
        let configuration = "{ ... }: { imports = [ ./hardware-configuration.nix ]; }";
        let hardware = "{ ... }: { }";
        write(&dir.path().join(CONFIGURATION), configuration);
        write(&dir.path().join(HARDWARE), hardware);

        let repo = scan(dir.path(), Some("phone")).unwrap();
        let plan = migration_plan(&repo, "phone", "x86_64-linux").unwrap();
        assert_eq!(plan.nixpkgs, DEFAULT_NIXPKGS);
        let staging = tempdir().unwrap();
        let staged = stage_migration_in(&plan, staging.path(), 1000, 100).unwrap();

        let bytes = fs::read(&staged.path).unwrap();
        let file: MigrationPlanFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(file.version, MIGRATION_PLAN_VERSION);
        assert_eq!(file.uid, 1000);
        assert_eq!(file.host, "phone");
        assert_eq!(file.system, "x86_64-linux");
        assert_eq!(file.created, 100);
        assert_eq!(file.expires, 100 + MIGRATION_PLAN_TTL_SECONDS);
        assert_eq!(file.operations.len(), 2);
        assert_eq!(file.operations[0].from, CONFIGURATION);
        assert_eq!(file.operations[0].to, "hosts/phone/configuration.nix");
        assert_eq!(
            file.operations[0].sha256,
            sha256_bytes(configuration.as_bytes())
        );
        assert_eq!(file.operations[1].sha256, sha256_bytes(hardware.as_bytes()));

        let mode = fs::metadata(&staged.path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn supports_a_local_channel_input() {
        let dir = tempdir().unwrap();
        write(&dir.path().join(CONFIGURATION), "{ ... }: { }");
        let repo = scan(dir.path(), Some("phone")).unwrap();
        let nixpkgs = "path:/nix/var/nix/profiles/per-user/root/channels/nixos";
        let plan = migration_plan_with_nixpkgs(&repo, "phone", "x86_64-linux", nixpkgs).unwrap();
        assert_eq!(plan.nixpkgs, nixpkgs);
        assert!(plan.operations.iter().any(|operation| matches!(
            operation,
            FileOperation::Create { path, contents }
                if path == FLAKE && contents.contains(nixpkgs)
        )));
        assert!(safe_nixpkgs(nixpkgs));
        assert!(!safe_nixpkgs("path:/etc/passwd"));
        assert!(!safe_nixpkgs("github:NixOS/nixpkgs/../../secret"));
    }

    #[test]
    fn refuses_to_stage_over_an_existing_destination() {
        let dir = tempdir().unwrap();
        write(&dir.path().join(CONFIGURATION), "{ ... }: { }");
        let repo = scan(dir.path(), Some("phone")).unwrap();
        let plan = migration_plan(&repo, "phone", "x86_64-linux").unwrap();
        write(
            &dir.path().join("hosts/phone/configuration.nix"),
            "{ ... }: { }",
        );

        let staging = tempdir().unwrap();
        let error = stage_migration_in(&plan, staging.path(), 1000, 100).unwrap_err();
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn refuses_nonstandard_flakes_for_now() {
        let dir = tempdir().unwrap();
        write(&dir.path().join(FLAKE), "{ outputs = _: { }; }");
        write(&dir.path().join(CONFIGURATION), "{ ... }: { }");

        let repo = scan(dir.path(), Some("phone")).unwrap();
        let error = migration_plan(&repo, "phone", "x86_64-linux").unwrap_err();
        assert!(error
            .to_string()
            .contains("flake migration is not implemented"));
    }
}
