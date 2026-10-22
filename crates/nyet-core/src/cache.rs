use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{OptionSpec, Result};

const SCHEMA: u32 = 2;
static WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    schema: u32,
    flake: String,
    host: String,
    fingerprint: String,
    options: Vec<OptionSpec>,
}

pub fn default_cache_dir() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("NYET_CACHE_DIR") {
        return Some(PathBuf::from(directory));
    }
    if let Some(directory) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(directory).join("nyet"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("nyet"))
}

pub(crate) fn load(
    directory: &Path,
    flake: &Path,
    host: &str,
    fingerprint: &str,
) -> Option<Vec<OptionSpec>> {
    let path = cache_path(directory, fingerprint);
    let bytes = std::fs::read(path).ok()?;
    let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
    if entry.schema != SCHEMA
        || entry.flake != flake.to_string_lossy()
        || entry.host != host
        || entry.fingerprint != fingerprint
    {
        return None;
    }
    Some(entry.options)
}

pub(crate) fn store(
    directory: &Path,
    flake: &Path,
    host: &str,
    fingerprint: &str,
    options: &[OptionSpec],
) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let entry = CacheEntry {
        schema: SCHEMA,
        flake: flake.to_string_lossy().into_owned(),
        host: host.to_string(),
        fingerprint: fingerprint.to_string(),
        options: options.to_vec(),
    };
    let bytes = serde_json::to_vec(&entry)?;
    let path = cache_path(directory, fingerprint);
    let temporary = directory.join(format!(
        ".index-{}-{}.tmp",
        std::process::id(),
        WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

pub(crate) fn fingerprint(flake: &Path, host: &str) -> String {
    let mut hash = Fnv::new();
    hash.write(flake.to_string_lossy().as_bytes());
    hash.write(host.as_bytes());

    let mut files = BTreeSet::new();
    collect_files(flake, &mut files);
    for file in files {
        if let Ok(relative) = file.strip_prefix(flake) {
            hash.write(relative.to_string_lossy().as_bytes());
        }
        if let Ok(bytes) = std::fs::read(&file) {
            hash.write(&bytes);
        }
    }

    format!("{:016x}", hash.finish())
}

fn cache_path(directory: &Path, fingerprint: &str) -> PathBuf {
    directory.join(format!("index-{fingerprint}.json"))
}

fn collect_files(directory: &Path, files: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "flake.lock" || path.extension().is_some_and(|extension| extension == "nix")
            {
                files.insert(path);
            }
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(
                name.as_ref(),
                ".git" | ".direnv" | "target" | "result" | "node_modules"
            ) {
                continue;
            }
            collect_files(&path, files);
        } else if file_type.is_file() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "flake.lock" || path.extension().is_some_and(|extension| extension == "nix")
            {
                files.insert(path);
            }
        }
    }
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OptionPath, Value};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("nyet-cache-{}-{nanos}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn option() -> OptionSpec {
        OptionSpec {
            path: OptionPath::parse("services.openssh.enable").unwrap(),
            type_name: "boolean".to_string(),
            description: String::new(),
            default_text: None,
            default: Some(Value::Bool(false)),
            read_only: false,
            internal: false,
            visible: true,
            declarations: Vec::new(),
            defined: false,
            definition_files: Vec::new(),
        }
    }

    #[test]
    fn fingerprint_changes_with_the_configuration() {
        let directory = temporary_directory("fingerprint");
        std::fs::write(directory.join("flake.lock"), "{}").unwrap();
        std::fs::write(directory.join("configuration.nix"), "{ }").unwrap();

        let first = fingerprint(&directory, "basic");
        std::fs::write(
            directory.join("configuration.nix"),
            "{ networking.hostName = \"x\"; }",
        )
        .unwrap();
        let second = fingerprint(&directory, "basic");

        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn round_trips_an_index_entry() {
        let directory = temporary_directory("roundtrip");
        let flake = PathBuf::from("/tmp/example");
        let key = "0123456789abcdef";
        let options = vec![option()];

        store(&directory, &flake, "basic", key, &options).unwrap();
        let loaded = load(&directory, &flake, "basic", key).unwrap();

        assert_eq!(loaded, options);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn ignores_a_stale_entry() {
        let directory = temporary_directory("stale");
        let flake = PathBuf::from("/tmp/example");

        store(&directory, &flake, "basic", "aaaaaaaaaaaaaaaa", &[option()]).unwrap();

        assert!(load(&directory, &flake, "basic", "bbbbbbbbbbbbbbbb").is_none());
        let _ = std::fs::remove_dir_all(directory);
    }
}
