use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rnix::{Root, SyntaxKind};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const SKIP_DIRS: &[&str] = &[".git", ".direnv", "node_modules", "result", "target"];
const CONFIGURATION: &str = "configuration.nix";
const HARDWARE: &str = "hardware-configuration.nix";
const FLAKE: &str = "flake.nix";

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
    pub operations: Vec<FileOperation>,
    pub warnings: Vec<String>,
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
    if repo.standard {
        return Ok(MigrationPlan {
            root: repo.root.clone(),
            kind: repo.kind,
            host: host.to_string(),
            system: system.to_string(),
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

    match repo.kind {
        RepoKind::Classic => classic_plan(repo, host, system),
        RepoKind::Flake => Err(Error::Usage(
            "this flake is not in the standard layout; flake migration is not implemented yet"
                .to_string(),
        )),
    }
}

fn classic_plan(repo: &Repo, host: &str, system: &str) -> Result<MigrationPlan> {
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
        contents: flake_text(host, system),
    });

    Ok(MigrationPlan {
        root: repo.root.clone(),
        kind: repo.kind,
        host: host.to_string(),
        system: system.to_string(),
        operations,
        warnings,
    })
}

fn flake_text(host: &str, system: &str) -> String {
    format!(
        r#"{{
  description = "NixOS configuration for {host}";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

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
