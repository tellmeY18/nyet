use std::collections::BTreeMap;

use crate::{Error, Intent, IntentOp, Result};

enum Entry {
    Scalar(String),
    List(BTreeMap<String, String>),
}

pub fn render_module(intents: &[Intent]) -> Result<String> {
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();

    for intent in intents {
        let path = intent.path.as_str();
        match intent.op {
            IntentOp::Set => {
                if matches!(entries.get(path), Some(Entry::List(_))) {
                    return Err(Error::InvalidValue(format!(
                        "{path} has both set and list intents"
                    )));
                }
                let value = intent.value.as_ref().ok_or_else(|| {
                    Error::InvalidValue(format!("{} requires a value", intent.path))
                })?;
                entries.insert(path.to_string(), Entry::Scalar(value.to_nix()));
            }
            IntentOp::Reset => {
                entries.remove(path);
            }
            IntentOp::ListAdd => {
                if matches!(entries.get(path), Some(Entry::Scalar(_))) {
                    return Err(Error::InvalidValue(format!(
                        "{path} has both set and list intents"
                    )));
                }
                let value = intent.value.as_ref().ok_or_else(|| {
                    Error::InvalidValue(format!("{} requires a value", intent.path))
                })?;
                let rendered = value.to_nix();
                match entries.entry(path.to_string()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(Entry::List(BTreeMap::from([(rendered.clone(), rendered)])));
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let Entry::List(items) = entry.get_mut() else {
                            return Err(Error::InvalidValue(format!(
                                "{path} has both set and list intents"
                            )));
                        };
                        items.insert(rendered.clone(), rendered);
                    }
                }
            }
            IntentOp::ListRemove => {
                if matches!(entries.get(path), Some(Entry::Scalar(_))) {
                    return Err(Error::InvalidValue(format!(
                        "{path} has both set and list intents"
                    )));
                }
                let value = intent.value.as_ref().ok_or_else(|| {
                    Error::InvalidValue(format!("{} requires a value", intent.path))
                })?;
                let rendered = value.to_nix();
                if let Some(Entry::List(items)) = entries.get_mut(path) {
                    items.remove(&rendered);
                    if items.is_empty() {
                        entries.remove(path);
                    }
                }
            }
        }
    }

    let mut module = String::from("{ config, lib, pkgs, ... }:\n{\n");
    for (path, entry) in entries {
        module.push_str("  ");
        module.push_str(&path);
        module.push_str(" = ");
        match entry {
            Entry::Scalar(value) => module.push_str(&value),
            Entry::List(items) => {
                if items.is_empty() {
                    module.push_str("[ ]");
                } else {
                    module.push_str("[ ");
                    module.push_str(&items.into_values().collect::<Vec<_>>().join(" "));
                    module.push_str(" ]");
                }
            }
        }
        module.push_str(";\n");
    }
    module.push_str("}\n");
    Ok(module)
}

pub fn unified_diff(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = format!("--- {old_name}\n+++ {new_name}\n");
    for (operation, line) in diff_operations(&old_lines, &new_lines) {
        diff.push(operation);
        diff.push(' ');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn diff_operations<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(char, &'a str)> {
    let old_len = old.len();
    let new_len = new.len();
    let mut lengths = vec![vec![0usize; new_len + 1]; old_len + 1];

    for old_index in (0..old_len).rev() {
        for new_index in (0..new_len).rev() {
            lengths[old_index][new_index] = if old[old_index] == new[new_index] {
                lengths[old_index + 1][new_index + 1] + 1
            } else {
                lengths[old_index + 1][new_index].max(lengths[old_index][new_index + 1])
            };
        }
    }

    let mut operations = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;

    while old_index < old_len && new_index < new_len {
        if old[old_index] == new[new_index] {
            operations.push((' ', old[old_index]));
            old_index += 1;
            new_index += 1;
        } else if lengths[old_index + 1][new_index] >= lengths[old_index][new_index + 1] {
            operations.push(('-', old[old_index]));
            old_index += 1;
        } else {
            operations.push(('+', new[new_index]));
            new_index += 1;
        }
    }

    while old_index < old_len {
        operations.push(('-', old[old_index]));
        old_index += 1;
    }
    while new_index < new_len {
        operations.push(('+', new[new_index]));
        new_index += 1;
    }

    operations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OptionPath, Value};

    fn intent(path: &str, value: Value) -> Intent {
        Intent::set(OptionPath::parse(path).unwrap(), value)
    }

    fn list_add(path: &str, value: Value) -> Intent {
        Intent::list_add(OptionPath::parse(path).unwrap(), value)
    }

    fn list_remove(path: &str, value: Value) -> Intent {
        Intent::list_remove(OptionPath::parse(path).unwrap(), value)
    }

    #[test]
    fn renders_a_deterministic_module() {
        let intents = [
            intent("services.openssh.enable", Value::Bool(true)),
            intent("networking.hostName", Value::String("nyet".to_string())),
        ];

        let module = render_module(&intents).unwrap();
        let golden = include_str!("../../../tests/golden/enable-ssh.nix");
        assert_eq!(module, golden);
    }

    #[test]
    fn reset_removes_an_assignment() {
        let intents = [
            intent("services.openssh.enable", Value::Bool(true)),
            Intent::reset(OptionPath::parse("services.openssh.enable").unwrap()),
        ];

        assert_eq!(
            render_module(&intents).unwrap(),
            "{ config, lib, pkgs, ... }:\n{\n}\n"
        );
    }

    #[test]
    fn renders_list_membership() {
        let intents = [
            list_add("boot.kernelModules", Value::String("kvm-intel".into())),
            list_add("boot.kernelModules", Value::String("loop".into())),
            list_add("boot.kernelModules", Value::String("kvm-intel".into())),
        ];

        assert_eq!(
            render_module(&intents).unwrap(),
            "{ config, lib, pkgs, ... }:\n{\n  boot.kernelModules = [ \"kvm-intel\" \"loop\" ];\n}\n"
        );
    }

    #[test]
    fn removing_every_list_member_omits_the_assignment() {
        let intents = [
            list_add("boot.kernelModules", Value::String("loop".into())),
            list_remove("boot.kernelModules", Value::String("loop".into())),
        ];

        assert_eq!(
            render_module(&intents).unwrap(),
            "{ config, lib, pkgs, ... }:\n{\n}\n"
        );
    }

    #[test]
    fn rejects_mixed_set_and_list_intents_for_one_path() {
        let path = OptionPath::parse("boot.kernelModules").unwrap();
        let intents = [
            list_add("boot.kernelModules", Value::String("loop".into())),
            Intent::set(path, Value::List(Vec::new())),
        ];

        assert!(render_module(&intents).is_err());
    }

    #[test]
    fn diffs_additions_and_removals() {
        let diff = unified_diff("a\nb\n", "a\nc\n", "old.nix", "new.nix");
        assert_eq!(diff, "--- old.nix\n+++ new.nix\n  a\n- b\n+ c\n");
    }

    #[test]
    fn diffs_an_empty_base() {
        let diff = unified_diff(
            "",
            "{ config, lib, pkgs, ... }:\n{\n}\n",
            "/dev/null",
            "nyet.nix",
        );
        assert_eq!(
            diff,
            "--- /dev/null\n+++ nyet.nix\n+ { config, lib, pkgs, ... }:\n+ {\n+ }\n"
        );
    }
}
