use serde::{Deserialize, Serialize};

use crate::validate::{validate, Warning};
use crate::{render_module, unified_diff, Intent, OptionSpec, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub intents: Vec<Intent>,
    pub module: String,
    pub diff: String,
    pub warnings: Vec<Warning>,
}

pub fn build_plan(
    options: &[OptionSpec],
    intents: &[Intent],
    base: &str,
    base_name: &str,
) -> Result<Plan> {
    let warnings = validate(intents, options)?;
    let module = render_module(intents)?;
    let diff = unified_diff(base, &module, base_name, "nyet.nix");
    Ok(Plan {
        intents: intents.to_vec(),
        module,
        diff,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OptionPath, Value};

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

    #[test]
    fn builds_a_validated_plan() {
        let options = [spec("services.openssh.enable", "boolean")];
        let intents = [Intent::set(
            OptionPath::parse("services.openssh.enable").unwrap(),
            Value::Bool(true),
        )];

        let plan = build_plan(&options, &intents, "", "/dev/null").unwrap();

        assert_eq!(
            plan.module,
            "{ config, lib, pkgs, ... }:\n{\n  services.openssh.enable = true;\n}\n"
        );
        assert!(plan.diff.contains("+   services.openssh.enable = true;"));
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn refuses_an_invalid_plan() {
        let options = [spec("services.openssh.enable", "boolean")];
        let intents = [Intent::set(
            OptionPath::parse("services.openssh.enable").unwrap(),
            Value::String("yes".into()),
        )];

        assert!(build_plan(&options, &intents, "", "/dev/null").is_err());
    }
}
