use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(String),
    Path(String),
    Package(String),
    List(Vec<Value>),
    Attrs(BTreeMap<String, Value>),
    Null,
    Expr(String),
}

impl Value {
    pub fn from_json(json: &Json, type_name: &str) -> Self {
        let hint = type_name.to_ascii_lowercase();
        match json {
            Json::Null => Self::Null,
            Json::Bool(value) => Self::Bool(*value),
            Json::Number(number) => {
                if hint.contains("float") {
                    number.as_f64().map(Self::Float).unwrap_or(Self::Null)
                } else if let Some(value) = number.as_i64() {
                    Self::Int(value)
                } else {
                    number.as_f64().map(Self::Float).unwrap_or(Self::Null)
                }
            }
            Json::String(value) => {
                if hint.contains("package") {
                    Self::Package(value.clone())
                } else if hint.contains("path") {
                    Self::Path(value.clone())
                } else if hint.contains("enum") || hint.contains("one of") {
                    Self::Enum(value.clone())
                } else {
                    Self::String(value.clone())
                }
            }
            Json::Array(items) => {
                let item_hint = if hint.contains("package") {
                    "package"
                } else if hint.contains("path") {
                    "path"
                } else {
                    ""
                };
                Self::List(
                    items
                        .iter()
                        .map(|item| Self::from_json(item, item_hint))
                        .collect(),
                )
            }
            Json::Object(map) => {
                if let Some(Json::String(out_path)) = map.get("outPath") {
                    return Self::Package(out_path.clone());
                }
                Self::Attrs(
                    map.iter()
                        .map(|(key, value)| (key.clone(), Self::from_json(value, "")))
                        .collect(),
                )
            }
        }
    }

    pub fn parse_cli(input: &str) -> Self {
        match serde_json::from_str::<Json>(input) {
            Ok(json) => Self::from_json(&json, ""),
            Err(_) => Self::String(input.to_string()),
        }
    }

    pub fn to_nix(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => float_literal(*value),
            Self::String(value) | Self::Enum(value) | Self::Path(value) => quote(value),
            Self::Package(value) | Self::Expr(value) => value.clone(),
            Self::List(items) => {
                if items.is_empty() {
                    "[ ]".to_string()
                } else {
                    format!(
                        "[ {} ]",
                        items.iter().map(Self::to_nix).collect::<Vec<_>>().join(" ")
                    )
                }
            }
            Self::Attrs(entries) => {
                if entries.is_empty() {
                    "{ }".to_string()
                } else {
                    format!(
                        "{{ {} }}",
                        entries
                            .iter()
                            .map(|(key, value)| format!("{} = {};", attr_name(key), value.to_nix()))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                }
            }
            Self::Null => "null".to_string(),
        }
    }
}

pub(crate) fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '$' if characters.peek() == Some(&'{') => {
                quoted.push_str("\\$");
            }
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn attr_name(name: &str) -> String {
    let mut characters = name.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    let valid_rest = name.chars().skip(1).all(|character| {
        character.is_ascii_alphanumeric()
            || character == '_'
            || character == '\''
            || character == '-'
    });
    if valid_first && valid_rest {
        name.to_string()
    } else {
        quote(name)
    }
}

fn float_literal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_typed_scalars() {
        assert_eq!(Value::from_json(&json!(true), "boolean"), Value::Bool(true));
        assert_eq!(
            Value::from_json(&json!(22), "signed integer"),
            Value::Int(22)
        );
        assert_eq!(
            Value::from_json(&json!("strict"), "one of \"strict\", \"loose\""),
            Value::Enum("strict".to_string())
        );
        assert_eq!(
            Value::from_json(&json!("/nix/store/thing"), "path"),
            Value::Path("/nix/store/thing".to_string())
        );
    }

    #[test]
    fn reads_a_package_from_a_derivation_object() {
        let value = Value::from_json(&json!({"outPath": "/nix/store/abc-ripgrep"}), "package");
        assert_eq!(value, Value::Package("/nix/store/abc-ripgrep".to_string()));
    }

    #[test]
    fn renders_nix_literals() {
        assert_eq!(Value::Bool(true).to_nix(), "true");
        assert_eq!(Value::Int(3).to_nix(), "3");
        assert_eq!(Value::Float(1.0).to_nix(), "1.0");
        assert_eq!(Value::Float(1.5).to_nix(), "1.5");
        assert_eq!(Value::Null.to_nix(), "null");
        assert_eq!(
            Value::String("a\"b\\c".to_string()).to_nix(),
            "\"a\\\"b\\\\c\""
        );
        assert_eq!(
            Value::String("${escape}".to_string()).to_nix(),
            "\"\\${escape}\""
        );
        assert_eq!(
            Value::List(vec![Value::Bool(true), Value::Int(2)]).to_nix(),
            "[ true 2 ]"
        );
        assert_eq!(Value::List(Vec::new()).to_nix(), "[ ]");
        assert_eq!(
            Value::Attrs(BTreeMap::from([(
                "a b".to_string(),
                Value::String("x".to_string())
            )]))
            .to_nix(),
            "{ \"a b\" = \"x\"; }"
        );
    }

    #[test]
    fn parses_cli_values() {
        assert_eq!(Value::parse_cli("true"), Value::Bool(true));
        assert_eq!(Value::parse_cli("42"), Value::Int(42));
        assert_eq!(
            Value::parse_cli("\"quoted\""),
            Value::String("quoted".to_string())
        );
        assert_eq!(
            Value::parse_cli("openssh"),
            Value::String("openssh".to_string())
        );
        assert_eq!(
            Value::parse_cli("[1, 2]"),
            Value::List(vec![Value::Int(1), Value::Int(2)])
        );
    }
}
