use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OptionPath(String);

impl OptionPath {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() {
            return Err(Error::InvalidPath(value));
        }
        if value.split('.').any(|segment| segment.is_empty()) {
            return Err(Error::InvalidPath(value));
        }
        if value.chars().any(char::is_whitespace) {
            return Err(Error::InvalidPath(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('.')
    }
}

impl fmt::Display for OptionPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for OptionPath {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_nested_path() {
        let path = OptionPath::parse("services.openssh.enable").unwrap();
        assert_eq!(path.as_str(), "services.openssh.enable");
        assert_eq!(path.segments().count(), 3);
    }

    #[test]
    fn rejects_empty_and_malformed_paths() {
        for value in ["", ".", "services.", ".enable", "services..enable", "a b"] {
            assert!(
                OptionPath::parse(value).is_err(),
                "{value} should be rejected"
            );
        }
    }
}
