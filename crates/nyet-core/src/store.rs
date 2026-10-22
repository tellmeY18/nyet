use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Intent, IntentOp, OptionPath, Result, Value};

const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    intents: Vec<Intent>,
}

#[derive(Debug, Clone)]
pub struct IntentStore {
    path: PathBuf,
    intents: Vec<Intent>,
}

pub fn default_state_dir() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("NYET_STATE_DIR") {
        return Some(PathBuf::from(directory));
    }
    if let Some(directory) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(directory).join("nyet"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/nyet"))
}

impl IntentStore {
    pub fn open(directory: &Path, flake: &Path, host: &str) -> Result<Self> {
        let path = directory
            .join("intents")
            .join(format!("{}.json", project_key(flake, host)));
        let intents = match std::fs::read(&path) {
            Ok(bytes) => {
                let file: StoreFile = serde_json::from_slice(&bytes)?;
                if file.version != VERSION {
                    return Err(Error::Usage(format!(
                        "unsupported intent store version {} in {}",
                        file.version,
                        path.display()
                    )));
                }
                file.intents
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, intents })
    }

    pub fn intents(&self) -> &[Intent] {
        &self.intents
    }

    pub fn set(&mut self, intent: Intent) -> Result<()> {
        self.intents.retain(|existing| existing.path != intent.path);
        self.intents.push(intent);
        self.save()
    }

    pub fn reset(&mut self, path: OptionPath) -> Result<()> {
        self.set(Intent::reset(path))
    }

    pub fn list_add(&mut self, intent: Intent) -> Result<()> {
        if self
            .intents
            .iter()
            .any(|existing| existing.path == intent.path && existing.op == IntentOp::Set)
        {
            return Err(Error::InvalidValue(format!(
                "{} already has a set intent; reset it before adding list members",
                intent.path
            )));
        }
        self.intents
            .retain(|existing| !(existing.path == intent.path && existing.op == IntentOp::Reset));
        let duplicate = self.intents.iter().any(|existing| {
            existing.path == intent.path
                && existing.op == IntentOp::ListAdd
                && existing.value == intent.value
        });
        if !duplicate {
            self.intents.push(intent);
            self.save()?;
        }
        Ok(())
    }

    pub fn list_remove(&mut self, path: OptionPath, value: Value) -> Result<()> {
        let before = self.intents.len();
        self.intents.retain(|existing| {
            !(existing.path == path
                && existing.op == IntentOp::ListAdd
                && existing.value.as_ref() == Some(&value))
        });
        if self.intents.len() == before {
            return Err(Error::Usage(format!(
                "{path} has no pending list entry {} to remove",
                value.to_nix()
            )));
        }
        self.save()
    }

    pub fn clear(&mut self) -> Result<usize> {
        let count = self.intents.len();
        self.intents.clear();
        self.save()?;
        Ok(count)
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&StoreFile {
            version: VERSION,
            intents: self.intents.clone(),
        })?;
        let temporary = self
            .path
            .with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

fn project_key(flake: &Path, host: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in flake
        .to_string_lossy()
        .bytes()
        .chain(std::iter::once(0))
        .chain(host.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("nyet-store-{}-{nanos}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn path(value: &str) -> OptionPath {
        OptionPath::parse(value).unwrap()
    }

    #[test]
    fn stores_and_reloads_intents() {
        let directory = temporary_directory("reload");
        let flake = PathBuf::from("/tmp/example");

        let mut store = IntentStore::open(&directory, &flake, "basic").unwrap();
        store
            .set(Intent::set(
                path("services.openssh.enable"),
                Value::Bool(true),
            ))
            .unwrap();
        store
            .set(Intent::set(
                path("networking.hostName"),
                Value::String("nyet".into()),
            ))
            .unwrap();

        let reloaded = IntentStore::open(&directory, &flake, "basic").unwrap();
        assert_eq!(reloaded.intents().len(), 2);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn setting_a_path_replaces_its_previous_intent() {
        let directory = temporary_directory("replace");
        let flake = PathBuf::from("/tmp/example");
        let mut store = IntentStore::open(&directory, &flake, "basic").unwrap();

        store
            .set(Intent::set(
                path("services.openssh.enable"),
                Value::Bool(true),
            ))
            .unwrap();
        store
            .set(Intent::set(
                path("services.openssh.enable"),
                Value::Bool(false),
            ))
            .unwrap();

        assert_eq!(store.intents().len(), 1);
        assert_eq!(store.intents()[0].value, Some(Value::Bool(false)));
    }

    #[test]
    fn adds_and_removes_list_members() {
        let directory = temporary_directory("list");
        let flake = PathBuf::from("/tmp/example");
        let mut store = IntentStore::open(&directory, &flake, "basic").unwrap();
        let modules = path("boot.kernelModules");

        store
            .list_add(Intent::list_add(
                modules.clone(),
                Value::String("loop".into()),
            ))
            .unwrap();
        store
            .list_add(Intent::list_add(
                modules.clone(),
                Value::String("loop".into()),
            ))
            .unwrap();
        assert_eq!(store.intents().len(), 1);

        store
            .list_remove(modules.clone(), Value::String("loop".into()))
            .unwrap();
        assert!(store.intents().is_empty());
        assert!(store
            .list_remove(modules, Value::String("loop".into()))
            .is_err());
    }

    #[test]
    fn clears_every_intent() {
        let directory = temporary_directory("clear");
        let flake = PathBuf::from("/tmp/example");
        let mut store = IntentStore::open(&directory, &flake, "basic").unwrap();
        store
            .set(Intent::set(
                path("services.openssh.enable"),
                Value::Bool(true),
            ))
            .unwrap();

        assert_eq!(store.clear().unwrap(), 1);
        assert!(store.intents().is_empty());
    }
}
