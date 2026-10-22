use serde::{Deserialize, Serialize};

use crate::{OptionPath, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub path: OptionPath,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub choices: Option<Vec<Value>>,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "defaultText", default)]
    pub default_text: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(rename = "readOnly", default)]
    pub read_only: bool,
    #[serde(default)]
    pub internal: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub declarations: Vec<String>,
    #[serde(default)]
    pub defined: bool,
    #[serde(rename = "definitionFiles", default)]
    pub definition_files: Vec<String>,
}

impl OptionSpec {
    pub fn is_list(&self) -> bool {
        self.type_name.to_ascii_lowercase().contains("list")
    }

    pub fn coerce_value(&self, value: Value) -> Value {
        coerce(value, &self.type_name)
    }

    pub fn coerce_element(&self, value: Value) -> Value {
        list_element_type(&self.type_name)
            .map(|element| coerce(value.clone(), element))
            .unwrap_or(value)
    }
}

pub(crate) fn list_element_type(type_name: &str) -> Option<&str> {
    let lower = type_name.to_ascii_lowercase();
    let index = lower.find("list of ")?;
    Some(type_name[index + "list of ".len()..].trim())
}

pub(crate) fn choices_for(type_name: &str) -> Option<Vec<Value>> {
    let lower = type_name.trim().to_ascii_lowercase();
    if lower == "boolean" {
        return Some(vec![Value::Bool(true), Value::Bool(false)]);
    }

    let index = lower.find("one of ")?;
    let values = quoted_values(&type_name[index + "one of ".len()..]);
    if values.is_empty() {
        return None;
    }

    let mut choices = Vec::with_capacity(values.len() + 1);
    if lower[..index].contains("null or") {
        choices.push(Value::Null);
    }
    choices.extend(values.into_iter().map(Value::Enum));
    Some(choices)
}

fn quoted_values(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '"' {
            continue;
        }
        let mut value = String::new();
        while let Some(character) = characters.next() {
            match character {
                '\\' => {
                    if let Some(escaped) = characters.next() {
                        value.push(escaped);
                    }
                }
                '"' => {
                    values.push(value);
                    break;
                }
                _ => value.push(character),
            }
        }
    }
    values
}

fn coerce(value: Value, type_name: &str) -> Value {
    let lower = type_name.trim().to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("null or ") {
        return match value {
            Value::Null => Value::Null,
            other => coerce(other, inner),
        };
    }
    if let Some(element) = list_element_type(type_name) {
        return match value {
            Value::List(items) => Value::List(
                items
                    .into_iter()
                    .map(|item| coerce(item, element))
                    .collect(),
            ),
            other => other,
        };
    }
    if lower.starts_with("one of ") {
        return match value {
            Value::String(value) => Value::Enum(value),
            other => other,
        };
    }
    if lower.contains("package") {
        return match value {
            Value::String(value) => Value::Package(value),
            other => other,
        };
    }
    if lower.contains("path") {
        return match value {
            Value::String(value) => Value::Path(value),
            other => other,
        };
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(type_name: &str) -> OptionSpec {
        OptionSpec {
            path: OptionPath::parse("test.option").unwrap(),
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

    #[test]
    fn coerces_cli_strings_to_the_option_type() {
        assert_eq!(
            spec("package").coerce_value(Value::String("pkgs.hello".into())),
            Value::Package("pkgs.hello".into())
        );
        assert_eq!(
            spec("list of package")
                .coerce_value(Value::List(vec![Value::String("pkgs.hello".into())])),
            Value::List(vec![Value::Package("pkgs.hello".into())])
        );
        assert_eq!(
            spec("one of \"a\", \"b\"").coerce_value(Value::String("a".into())),
            Value::Enum("a".into())
        );
        assert_eq!(
            spec("path").coerce_value(Value::String("/etc/nixos".into())),
            Value::Path("/etc/nixos".into())
        );
        assert_eq!(
            spec("list of string").coerce_element(Value::String("x".into())),
            Value::String("x".into())
        );
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionView {
    pub found: bool,
    #[serde(default)]
    pub spec: Option<OptionSpec>,
    #[serde(default)]
    pub value: Option<Value>,
}

fn default_true() -> bool {
    true
}
