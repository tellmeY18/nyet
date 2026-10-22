use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn cache_directory() -> PathBuf {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            let directory = temporary_directory("cache");
            std::fs::create_dir_all(&directory).unwrap();
            directory
        })
        .clone()
}

fn nyet(arguments: &[&str]) -> Output {
    let state = temporary_directory("state");
    let output = nyet_in(&state, arguments);
    let _ = std::fs::remove_dir_all(state);
    output
}

fn nyet_fixture(arguments: &[&str]) -> Output {
    let state = temporary_directory("fixture-state");
    let output = nyet_fixture_in(&state, arguments);
    let _ = std::fs::remove_dir_all(state);
    output
}

fn nyet_fixture_in(state: &Path, arguments: &[&str]) -> Output {
    let fixture = fixture().display().to_string();
    let mut all = vec![
        "--flake".to_string(),
        fixture,
        "--host".to_string(),
        "basic".to_string(),
    ];
    all.extend(arguments.iter().map(|argument| argument.to_string()));
    nyet_strings(state, &all)
}

fn nyet_in(state: &Path, arguments: &[&str]) -> Output {
    let arguments: Vec<String> = arguments
        .iter()
        .map(|argument| argument.to_string())
        .collect();
    nyet_strings(state, &arguments)
}

fn nyet_strings(state: &Path, arguments: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nyet"))
        .env("NYET_CACHE_DIR", cache_directory())
        .env("NYET_STATE_DIR", state)
        .args(arguments)
        .output()
        .expect("nyet binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be utf-8")
}

#[test]
fn plans_a_set_intent_without_writing_anything() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&["plan", "--set", "services.openssh.enable=true"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("{ config, lib, pkgs, ... }:"));
    assert!(output.contains("services.openssh.enable = true;"));
    assert!(output.contains("+++ nyet.nix"));
}

#[test]
fn plans_json_output() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&["plan", "--json", "--set", "networking.hostName=nyet"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        value["module"],
        "{ config, lib, pkgs, ... }:\n{\n  networking.hostName = \"nyet\";\n}\n"
    );
    assert!(value["diff"]
        .as_str()
        .unwrap()
        .contains("+   networking.hostName"));
    assert!(value["warnings"].is_array());
}

#[test]
fn plans_against_a_base_file() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let base = temporary_file(
        "old.nix",
        "{ ... }:\n{\n  services.openssh.enable = false;\n}\n",
    );
    let base_argument = base.display().to_string();

    let output = nyet_fixture(&[
        "plan",
        "--base",
        &base_argument,
        "--set",
        "services.openssh.enable=true",
    ]);

    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("-   services.openssh.enable = false;"));
    assert!(output.contains("+   services.openssh.enable = true;"));

    let _ = std::fs::remove_file(base);
}

#[test]
fn rejects_a_malformed_set_argument() {
    let output = nyet_fixture(&["plan", "--set", "services.openssh.enable"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("PATH=VALUE"));
}

#[test]
fn stores_sets_and_plans_them() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let state = temporary_directory("persisted");

    let output = nyet_fixture_in(&state, &["set", "services.nginx.enable", "true"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = nyet_fixture_in(&state, &["intents", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let intents: Vec<serde_json::Value> = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0]["op"], "set");
    assert_eq!(intents[0]["path"], "services.nginx.enable");

    let output = nyet_fixture_in(&state, &["plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("services.nginx.enable = true;"));

    let _ = std::fs::remove_dir_all(state);
}

#[test]
fn resets_an_intent() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let state = temporary_directory("reset");

    assert!(
        nyet_fixture_in(&state, &["set", "services.nginx.enable", "true"])
            .status
            .success()
    );
    let output = nyet_fixture_in(&state, &["unset", "services.nginx.enable"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = nyet_fixture_in(&state, &["plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "{ config, lib, pkgs, ... }:\n{\n}\n--- diff ---\n--- /dev/null\n+++ nyet.nix\n+ { config, lib, pkgs, ... }:\n+ {\n+ }\n"
    );

    let _ = std::fs::remove_dir_all(state);
}

#[test]
fn adds_and_removes_list_entries() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let state = temporary_directory("list");

    let output = nyet_fixture_in(&state, &["add", "boot.kernelModules", "loop"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = nyet_fixture_in(&state, &["plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("boot.kernelModules = [ \"loop\" ];"));

    let output = nyet_fixture_in(&state, &["remove", "boot.kernelModules", "loop"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = nyet_fixture_in(&state, &["intents", "--json"]);
    let intents: Vec<serde_json::Value> = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(intents.is_empty());

    let _ = std::fs::remove_dir_all(state);
}

#[test]
fn adds_a_package_expression() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let state = temporary_directory("package");

    let output = nyet_fixture_in(&state, &["add", "environment.systemPackages", "pkgs.hello"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = nyet_fixture_in(&state, &["plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("environment.systemPackages = [ pkgs.hello ];"));

    let _ = std::fs::remove_dir_all(state);
}

#[test]
fn rejects_a_wrong_value_type() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&["set", "services.openssh.enable", "yes"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("expects boolean"));
}

#[test]
fn warns_about_a_definition_elsewhere() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&["plan", "--set", "services.openssh.enable=false"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("warning: services.openssh.enable is also defined in"));
}

#[test]
fn checks_a_candidate_module() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&["plan", "--check", "--set", "services.nginx.enable=true"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("check: ok (/nix/store/"));
}

#[test]
fn lists_options_from_the_fixture() {
    if !nix_is_available() {
        eprintln!("skipping fixture evaluation: nix is not available");
        return;
    }

    let output = nyet_fixture(&[
        "options",
        "--prefix",
        "services.openssh",
        "--query",
        "enable",
        "--json",
    ]);

    assert!(output.status.success(), "{}", stderr(&output));
    let options: Vec<serde_json::Value> = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(options.iter().any(|option| {
        option["path"] == serde_json::json!("services.openssh.enable")
            && option["type"] == serde_json::json!("boolean")
    }));
}

#[test]
fn requires_a_flake_for_options() {
    let output = nyet(&["options"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("--flake is required"));
}

fn nix_is_available() -> bool {
    Command::new("nix").arg("--version").output().is_ok()
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/basic")
}

fn temporary_directory(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("nyet-cli-{}-{nanos}-{name}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn temporary_file(name: &str, content: &str) -> PathBuf {
    let path = temporary_directory("file").join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn apply_and_rollback_reject_json() {
    let apply = nyet_fixture(&["apply", "--json"]);
    assert!(!apply.status.success());
    assert!(stderr(&apply).contains("apply does not support --json"));

    let rollback = nyet_fixture(&["rollback", "--json"]);
    assert!(!rollback.status.success());
    assert!(stderr(&rollback).contains("rollback does not support --json"));
}

#[test]
fn history_json_is_an_array() {
    let output = nyet(&["history", "--json"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert!(value.is_array());
}
