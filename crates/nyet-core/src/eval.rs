use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value as Json;

use crate::value::quote;
use crate::{cache, Error, OptionPath, OptionSpec, OptionView, Result, Value};

const OPTIONS_NIX: &str = include_str!("nix/options.nix");
const GET_NIX: &str = include_str!("nix/get.nix");
const DEFAULT_NIX: &str = include_str!("nix/default.nix");
const CHECK_NIX: &str = include_str!("nix/check.nix");

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub flake: PathBuf,
    pub host: String,
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Engine {
    config: EngineConfig,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self> {
        if config.host.is_empty()
            || !config.host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(Error::InvalidHost(config.host));
        }

        let flake = std::fs::canonicalize(&config.flake)?;
        Ok(Self {
            config: EngineConfig {
                flake,
                host: config.host,
                cache_dir: config.cache_dir,
            },
        })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn options(&self) -> Result<Vec<OptionSpec>> {
        let fingerprint = self
            .config
            .cache_dir
            .as_ref()
            .map(|_| cache::fingerprint(&self.config.flake, &self.config.host));

        if let (Some(directory), Some(fingerprint)) = (&self.config.cache_dir, &fingerprint) {
            if let Some(options) = cache::load(
                directory,
                &self.config.flake,
                &self.config.host,
                fingerprint,
            ) {
                return Ok(options);
            }
        }

        let options = self.options_uncached()?;
        if let (Some(directory), Some(fingerprint)) = (&self.config.cache_dir, &fingerprint) {
            let _ = cache::store(
                directory,
                &self.config.flake,
                &self.config.host,
                fingerprint,
                &options,
            );
        }
        Ok(options)
    }

    pub fn refresh_options(&self) -> Result<Vec<OptionSpec>> {
        let options = self.options_uncached()?;
        if let Some(directory) = &self.config.cache_dir {
            let fingerprint = cache::fingerprint(&self.config.flake, &self.config.host);
            let _ = cache::store(
                directory,
                &self.config.flake,
                &self.config.host,
                &fingerprint,
                &options,
            );
        }
        Ok(options)
    }

    fn options_uncached(&self) -> Result<Vec<OptionSpec>> {
        let value = self.eval(OPTIONS_NIX, &[])?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn check_module(&self, module: &str) -> Result<String> {
        let value = self.eval(CHECK_NIX, &[("module", module)])?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn get(&self, path: &OptionPath) -> Result<OptionView> {
        let value = self.eval(GET_NIX, &[("path", path.as_str())])?;
        let raw: RawView = serde_json::from_value(value)?;

        if !raw.found {
            return Err(Error::UnknownOption(path.to_string()));
        }

        let type_name = raw
            .spec
            .as_ref()
            .map(|spec| spec.type_name.clone())
            .unwrap_or_default();
        let value = if raw.value.is_null() {
            None
        } else {
            Some(Value::from_json(&raw.value, &type_name))
        };

        let mut view = OptionView {
            found: true,
            spec: raw.spec,
            value,
        };

        let needs_default = view
            .spec
            .as_ref()
            .is_some_and(|spec| spec.default.is_none() && spec.default_text.is_none());

        if needs_default {
            if let Ok(default) = self.eval(DEFAULT_NIX, &[("path", path.as_str())]) {
                if !default.is_null() {
                    if let Some(spec) = view.spec.as_mut() {
                        spec.default = Some(Value::from_json(&default, &spec.type_name));
                    }
                }
            }
        }

        Ok(view)
    }

    fn eval(&self, script: &str, extra: &[(&str, &str)]) -> Result<Json> {
        let directory = TempDir::new()?;
        let script_path = directory.path().join("expr.nix");
        std::fs::write(&script_path, script)?;

        let mut expression = format!(
            "(import {}) {{ flake = {}; host = {};",
            quote(&script_path.to_string_lossy()),
            quote(&self.config.flake.to_string_lossy()),
            quote(&self.config.host),
        );
        for (name, value) in extra {
            expression.push_str(&format!(" {name} = {};", quote(value)));
        }
        expression.push_str(" }");

        let output = Command::new("nix")
            .args(["eval", "--json", "--impure", "--expr"])
            .arg(&expression)
            .output()
            .map_err(|error| Error::NixUnavailable(error.to_string()))?;

        if !output.status.success() {
            return Err(Error::Nix(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }

        serde_json::from_slice(&output.stdout).map_err(Error::from)
    }
}

#[derive(Debug, Deserialize)]
struct RawView {
    found: bool,
    #[serde(default)]
    spec: Option<OptionSpec>,
    #[serde(default)]
    value: Json,
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("nyet-eval-{}-{nanos}-{count}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nix_is_available() -> bool {
        Command::new("nix").arg("--version").output().is_ok()
    }

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/basic")
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig {
            flake: fixture(),
            host: "basic".to_string(),
            cache_dir: None,
        })
        .unwrap()
    }

    #[test]
    fn indexes_options_from_the_fixture() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let options = engine().options().unwrap();
        let ssh = options
            .iter()
            .find(|option| option.path.as_str() == "services.openssh.enable")
            .expect("fixture should expose services.openssh.enable");

        assert_eq!(ssh.type_name, "boolean");
        assert!(!ssh.description.is_empty());
        assert!(!ssh.read_only);
    }

    #[test]
    fn reads_definition_provenance_for_a_selected_path() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let path = OptionPath::parse("services.openssh.enable").unwrap();
        let view = engine().get(&path).unwrap();
        let spec = view.spec.unwrap();

        assert!(spec.defined);
        assert!(!spec.definition_files.is_empty());
    }

    #[test]
    fn reads_a_current_value_and_default() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let path = OptionPath::parse("services.openssh.enable").unwrap();
        let view = engine().get(&path).unwrap();

        assert_eq!(view.value, Some(Value::Bool(true)));
        assert_eq!(
            view.spec.and_then(|spec| spec.default),
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn caches_the_option_index() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let directory = std::env::temp_dir().join(format!(
            "nyet-eval-cache-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let engine = Engine::new(EngineConfig {
            flake: fixture(),
            host: "basic".to_string(),
            cache_dir: Some(directory.clone()),
        })
        .unwrap();

        engine.options().unwrap();
        let entries = std::fs::read_dir(&directory).unwrap().count();
        assert_eq!(entries, 1);

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn checks_a_candidate_module() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let module = "{ ... }:\n{\n  services.nginx.enable = true;\n}\n";
        let drv = engine().check_module(module).unwrap();

        assert!(drv.starts_with("/nix/store/"));
        assert!(drv.ends_with(".drv"));
    }

    #[test]
    fn reports_an_unknown_option() {
        if !nix_is_available() {
            eprintln!("skipping fixture evaluation: nix is not available");
            return;
        }

        let path = OptionPath::parse("services.this-does-not-exist.enable").unwrap();
        assert!(matches!(engine().get(&path), Err(Error::UnknownOption(_))));
    }
}
