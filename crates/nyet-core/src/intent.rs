use serde::{Deserialize, Serialize};

use crate::{Error, OptionPath, Result, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentOp {
    Set,
    Reset,
    ListAdd,
    ListRemove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub op: IntentOp,
    pub path: OptionPath,
    #[serde(default)]
    pub value: Option<Value>,
}

impl Intent {
    pub fn set(path: OptionPath, value: Value) -> Self {
        Self {
            op: IntentOp::Set,
            path,
            value: Some(value),
        }
    }

    pub fn reset(path: OptionPath) -> Self {
        Self {
            op: IntentOp::Reset,
            path,
            value: None,
        }
    }

    pub fn list_add(path: OptionPath, value: Value) -> Self {
        Self {
            op: IntentOp::ListAdd,
            path,
            value: Some(value),
        }
    }

    pub fn list_remove(path: OptionPath, value: Value) -> Self {
        Self {
            op: IntentOp::ListRemove,
            path,
            value: Some(value),
        }
    }

    pub fn value(&self) -> Result<&Value> {
        match self.op {
            IntentOp::Set | IntentOp::ListAdd | IntentOp::ListRemove => self
                .value
                .as_ref()
                .ok_or_else(|| Error::InvalidValue(format!("{} requires a value", self.path))),
            IntentOp::Reset => Err(Error::InvalidValue(format!(
                "{} does not take a value",
                self.path
            ))),
        }
    }
}
