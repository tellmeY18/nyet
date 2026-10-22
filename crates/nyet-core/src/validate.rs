use std::fmt;

use serde::{Deserialize, Serialize};

use crate::option::list_element_type;
use crate::{Error, Intent, IntentOp, OptionPath, OptionSpec, Result, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    DefinedElsewhere {
        path: OptionPath,
        files: Vec<String>,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefinedElsewhere { path, files } => {
                write!(formatter, "{path} is also defined in")?;
                for file in files {
                    write!(formatter, "\n  - {file}")?;
                }
                Ok(())
            }
        }
    }
}

pub fn validate(intents: &[Intent], options: &[OptionSpec]) -> Result<Vec<Warning>> {
    let mut warnings = Vec::new();

    for intent in intents {
        let spec = options
            .iter()
            .find(|option| option.path == intent.path)
            .ok_or_else(|| Error::UnknownOption(intent.path.to_string()))?;

        if spec.read_only {
            return Err(Error::InvalidValue(format!("{} is read-only", intent.path)));
        }

        match intent.op {
            IntentOp::Set => {
                let value = intent.value()?;
                require_type(&intent.path, value, &spec.type_name)?;
                if spec.defined && !spec.definition_files.is_empty() {
                    warnings.push(Warning::DefinedElsewhere {
                        path: intent.path.clone(),
                        files: spec.definition_files.clone(),
                    });
                }
            }
            IntentOp::Reset => {
                if intent.value.is_some() {
                    return Err(Error::InvalidValue(format!(
                        "{} does not take a value",
                        intent.path
                    )));
                }
            }
            IntentOp::ListAdd | IntentOp::ListRemove => {
                let element_type = list_element_type(&spec.type_name).ok_or_else(|| {
                    Error::InvalidValue(format!("{} is not a list option", intent.path))
                })?;
                require_type(&intent.path, intent.value()?, element_type)?;
            }
        }
    }

    Ok(warnings)
}

fn require_type(path: &OptionPath, value: &Value, type_name: &str) -> Result<()> {
    if matches_type(value, type_name) {
        Ok(())
    } else {
        Err(Error::InvalidValue(format!(
            "{path} expects {type_name}, got {}",
            value.to_nix()
        )))
    }
}

fn matches_type(value: &Value, type_name: &str) -> bool {
    let lower = type_name.trim().to_ascii_lowercase();

    if lower == "null" {
        return matches!(value, Value::Null);
    }
    if let Some(inner) = lower.strip_prefix("null or ") {
        return match value {
            Value::Null => true,
            other => matches_type(other, inner),
        };
    }
    if let Some(element) = list_element_type(type_name) {
        return match value {
            Value::List(items) => items.iter().all(|item| matches_type(item, element)),
            _ => false,
        };
    }
    if lower.starts_with("one of ") {
        return match value {
            Value::String(value) | Value::Enum(value) => enum_values(type_name)
                .map(|values| values.iter().any(|candidate| candidate == value))
                .unwrap_or(true),
            _ => false,
        };
    }
    if lower.contains("attribute set") || lower.contains("submodule") || lower.contains("attrs") {
        matches!(value, Value::Attrs(_))
    } else if lower.contains("boolean") {
        matches!(value, Value::Bool(_))
    } else if lower.contains("float") {
        matches!(value, Value::Float(_) | Value::Int(_))
    } else if lower.contains("integer") {
        match value {
            Value::Int(value) => !lower.contains("unsigned") || *value >= 0,
            _ => false,
        }
    } else if lower.contains("string") {
        matches!(value, Value::String(_) | Value::Enum(_))
    } else if lower.contains("path") {
        matches!(value, Value::Path(_) | Value::String(_))
    } else if lower.contains("package") {
        matches!(value, Value::Package(_) | Value::Expr(_))
    } else if lower.contains("list") {
        matches!(value, Value::List(_))
    } else {
        true
    }
}

fn enum_values(type_name: &str) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut characters = type_name.chars();
    while let Some(character) = characters.next() {
        if character != '"' {
            continue;
        }
        let mut value = String::new();
        while let Some(character) = characters.next() {
            match character {
                '"' => break,
                '\\' => {
                    if let Some(escaped) = characters.next() {
                        value.push(escaped);
                    }
                }
                character => value.push(character),
            }
        }
        values.push(value);
    }
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(path: &str, type_name: &str) -> OptionSpec {
        OptionSpec {
            path: OptionPath::parse(path).unwrap(),
            type_name: type_name.to_string(),
            choices: None,
            description: String::new(),
            default_text: None,
            default: None,
            read_only: false,
            internal: false,
            visible: true,
            declarations: Vec::new(),
            defined: false,
            definition_files: Vec::new(),
        }
    }

    fn intent(path: &str, value: Value) -> Intent {
        Intent::set(OptionPath::parse(path).unwrap(), value)
    }

    #[test]
    fn accepts_a_matching_type() {
        let options = [spec("services.openssh.enable", "boolean")];
        let intents = [intent("services.openssh.enable", Value::Bool(true))];

        assert!(validate(&intents, &options).unwrap().is_empty());
    }

    #[test]
    fn rejects_a_mismatched_type() {
        let options = [spec("services.openssh.enable", "boolean")];
        let intents = [intent(
            "services.openssh.enable",
            Value::String("yes".into()),
        )];

        let error = validate(&intents, &options).unwrap_err();
        assert!(error.to_string().contains("expects boolean"));
    }

    #[test]
    fn rejects_an_unknown_option() {
        let intents = [intent("services.unknown.enable", Value::Bool(true))];

        assert!(matches!(
            validate(&intents, &[]),
            Err(Error::UnknownOption(_))
        ));
    }

    #[test]
    fn rejects_a_read_only_option() {
        let mut option = spec("system.stateVersion", "string");
        option.read_only = true;
        let intents = [intent("system.stateVersion", Value::String("24.05".into()))];

        assert!(validate(&intents, &[option]).is_err());
    }

    #[test]
    fn validates_enum_membership() {
        let options = [spec("services.x.enable", "one of \"strict\", \"loose\"")];
        let valid = [intent("services.x.enable", Value::Enum("strict".into()))];
        let invalid = [intent("services.x.enable", Value::Enum("other".into()))];

        assert!(validate(&valid, &options).unwrap().is_empty());
        assert!(validate(&invalid, &options).is_err());
    }

    #[test]
    fn requires_a_coerced_package_value() {
        let options = [spec("environment.systemPackages", "list of package")];
        let path = OptionPath::parse("environment.systemPackages").unwrap();
        let valid = [Intent::list_add(
            path.clone(),
            Value::Package("pkgs.hello".into()),
        )];
        let invalid = [Intent::list_add(path, Value::String("pkgs.hello".into()))];

        assert!(validate(&valid, &options).unwrap().is_empty());
        assert!(validate(&invalid, &options).is_err());
    }

    #[test]
    fn validates_list_elements() {
        let options = [spec("boot.kernelModules", "list of string")];
        let valid = [Intent::list_add(
            OptionPath::parse("boot.kernelModules").unwrap(),
            Value::String("loop".into()),
        )];
        let invalid = [Intent::list_add(
            OptionPath::parse("boot.kernelModules").unwrap(),
            Value::Bool(true),
        )];

        assert!(validate(&valid, &options).unwrap().is_empty());
        assert!(validate(&invalid, &options).is_err());
    }

    #[test]
    fn warns_when_the_option_is_defined_elsewhere() {
        let mut option = spec("services.openssh.enable", "boolean");
        option.defined = true;
        option.definition_files = vec!["/etc/nixos/configuration.nix".into()];
        let intents = [intent("services.openssh.enable", Value::Bool(false))];

        let warnings = validate(&intents, &[option]).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0]
            .to_string()
            .contains("/etc/nixos/configuration.nix"));
    }
}
