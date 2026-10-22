use std::fs;
#[cfg(target_os = "linux")]
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const PLAN_VERSION: u32 = 1;
const TRANSACTION_VERSION: u32 = 1;
const STATE_DIR: &str = "/var/lib/nyet";
const MODULE_PATH: &str = "/var/lib/nyet/nyet.nix";
const MODULE_PREVIOUS: &str = "/var/lib/nyet/nyet.nix.previous";
const WRAPPER_PATH: &str = "/var/lib/nyet/flake.nix";
const TRANSACTION_DIR: &str = "/var/lib/nyet/transactions";
const CONFIGURATION_PATH: &str = "/etc/nixos";
const MODULE_LIMIT: usize = 1024 * 1024;
const PLAN_LIFETIME: u64 = 600;

#[derive(Debug, Deserialize)]
struct PlanFile {
    version: u32,
    id: String,
    uid: u32,
    host: String,
    created: u64,
    expires: u64,
    module: String,
}

#[derive(Debug, Serialize)]
struct Transaction {
    version: u32,
    id: String,
    kind: &'static str,
    plan_id: Option<String>,
    uid: u32,
    started: u64,
    finished: u64,
    status: &'static str,
    before: Option<String>,
    after: Option<String>,
    message: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("nyet-helper: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments
        .next()
        .ok_or_else(|| usage().to_string())?
        .into_string()
        .map_err(|_| usage().to_string())?;

    match command.as_str() {
        "apply" => {
            let id = arguments
                .next()
                .ok_or_else(usage)?
                .into_string()
                .map_err(|_| usage().to_string())?;
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            apply(&id)
        }
        "rollback" => {
            if arguments.next().is_some() {
                return Err(usage().to_string());
            }
            ensure_root()?;
            rollback()
        }
        _ => Err(usage().to_string()),
    }
}

fn usage() -> &'static str {
    "usage: nyet-helper apply <plan-id> | nyet-helper rollback"
}

fn apply(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err("invalid plan id".to_string());
    }

    let _lock = lock()?;
    let uid = caller_uid()?;
    let path = plan_directory(uid).join(format!("{id}.json"));
    let plan = read_plan(&path, id, uid)?;
    if !safe_host(&plan.host) {
        return Err("invalid host in plan".to_string());
    }
    if !Path::new(CONFIGURATION_PATH).join("flake.nix").exists() {
        return Err(format!("{CONFIGURATION_PATH} does not contain a flake"));
    }

    let started = unix_time();
    let before = generation();
    let previous_module = read_optional(MODULE_PATH)?;
    let previous_wrapper = read_optional(WRAPPER_PATH)?;
    let previous_previous = previous_module
        .as_ref()
        .map(|module| module.as_bytes())
        .map(|module| (MODULE_PREVIOUS, module));

    if let Some((previous_path, module)) = previous_previous {
        write_atomic(Path::new(previous_path), module, 0o644)?;
    }

    write_atomic(
        Path::new(WRAPPER_PATH),
        wrapper(&plan.host).as_bytes(),
        0o644,
    )?;
    if let Err(error) = write_atomic(Path::new(MODULE_PATH), plan.module.as_bytes(), 0o644) {
        restore(WRAPPER_PATH, previous_wrapper.as_deref())?;
        return Err(error);
    }

    let result = rebuild(&plan.host);
    if result.is_err() {
        restore(MODULE_PATH, previous_module.as_deref())?;
        restore(WRAPPER_PATH, previous_wrapper.as_deref())?;
    }

    let after = generation();
    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "apply",
        plan_id: Some(plan.id.clone()),
        uid,
        started,
        finished: unix_time(),
        status: if result.is_ok() { "success" } else { "failure" },
        before,
        after,
        message: result.as_ref().err().cloned(),
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }

    result?;

    let _ = fs::remove_file(&path);
    Ok(())
}

fn rollback() -> Result<(), String> {
    let _lock = lock()?;
    let started = unix_time();
    let before = generation();
    let previous_module = read_optional(MODULE_PREVIOUS)?;
    let result = rebuild_rollback();
    let after = generation();

    if result.is_ok() {
        match previous_module {
            Some(module) => {
                write_atomic(Path::new(MODULE_PATH), module.as_bytes(), 0o644)?;
                let _ = fs::remove_file(MODULE_PREVIOUS);
            }
            None => {
                let _ = fs::remove_file(MODULE_PATH);
            }
        }
    }

    let transaction = Transaction {
        version: TRANSACTION_VERSION,
        id: transaction_id(started),
        kind: "rollback",
        plan_id: None,
        uid: caller_uid()?,
        started,
        finished: unix_time(),
        status: if result.is_ok() { "success" } else { "failure" },
        before,
        after,
        message: result.as_ref().err().cloned(),
    };
    if let Err(error) = write_transaction(&transaction) {
        eprintln!("nyet-helper: warning: cannot record transaction: {error}");
    }

    result
}

fn lock() -> Result<fs::File, String> {
    fs::create_dir_all(STATE_DIR).map_err(|error| format!("cannot create {STATE_DIR}: {error}"))?;
    fs::set_permissions(STATE_DIR, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot set permissions on {STATE_DIR}: {error}"))?;
    let path = Path::new(STATE_DIR).join("lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        return Err(format!(
            "cannot lock {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(file)
}

fn read_plan(path: &Path, id: &str, uid: u32) -> Result<PlanFile, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("plan must be a regular file".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != uid {
            return Err("plan is not owned by the authenticated user".to_string());
        }
        if metadata.mode() & 0o077 != 0 {
            return Err("plan permissions are too broad".to_string());
        }
    }

    let bytes = fs::read(path).map_err(|error| format!("cannot read plan {id}: {error}"))?;
    if bytes.len() > MODULE_LIMIT {
        return Err("plan is too large".to_string());
    }
    let plan: PlanFile =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid plan: {error}"))?;

    if plan.version != PLAN_VERSION {
        return Err("unsupported plan version".to_string());
    }
    if plan.id != id {
        return Err("plan id does not match".to_string());
    }
    if plan.uid != uid {
        return Err("plan does not belong to the authenticated user".to_string());
    }
    if plan.module.len() > MODULE_LIMIT {
        return Err("module is too large".to_string());
    }
    let now = unix_time();
    if plan.created > now.saturating_add(30) || plan.expires < now {
        return Err("plan has expired".to_string());
    }
    if plan.expires < plan.created || plan.expires - plan.created > PLAN_LIFETIME {
        return Err("plan lifetime is invalid".to_string());
    }
    validate_module(&plan.module)?;
    Ok(plan)
}

fn validate_module(text: &str) -> Result<(), String> {
    let mut lines = text.lines();
    if lines.next() != Some("{ config, lib, pkgs, ... }:") {
        return Err("module header is not allowed".to_string());
    }
    if lines.next() != Some("{") {
        return Err("module body must start with an attribute set".to_string());
    }

    let mut closed = false;
    for line in lines {
        if line == "}" {
            if closed {
                return Err("content after the module body".to_string());
            }
            closed = true;
            continue;
        }
        if closed {
            return Err("content after the module body".to_string());
        }
        if line.is_empty() || line.starts_with("  #") {
            return Err("module contains an unsupported line".to_string());
        }
        let assignment = line
            .strip_prefix("  ")
            .ok_or_else(|| "module line is not indented".to_string())?;
        let (path, value) = assignment
            .split_once(" = ")
            .ok_or_else(|| "module line is not an assignment".to_string())?;
        if !safe_path(path) {
            return Err(format!("unsafe option path {path:?}"));
        }
        let value = value
            .strip_suffix(';')
            .ok_or_else(|| "module assignment must end with ';'".to_string())?;
        let mut parser = Parser::new(value);
        parser.literal()?;
        parser.whitespace();
        if !parser.done() {
            return Err(format!("unexpected content in value for {path}"));
        }
    }

    if !closed {
        return Err("module body is not closed".to_string());
    }
    Ok(())
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            depth: 0,
        }
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("value is nested too deeply".to_string());
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn done(&self) -> bool {
        self.position == self.input.len()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.position..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(character) if character.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn expect(&mut self, character: char) -> Result<(), String> {
        if self.bump() == Some(character) {
            Ok(())
        } else {
            Err(format!("expected {character:?}"))
        }
    }

    fn literal(&mut self) -> Result<(), String> {
        self.whitespace();
        match self.peek() {
            Some('"') => self.string(),
            Some('[') => self.list(),
            Some('{') => self.attrs(),
            Some('t') => self.keyword("true"),
            Some('f') => self.keyword("false"),
            Some('n') => self.keyword("null"),
            Some('p') => self.package(),
            Some(character) if character == '-' || character.is_ascii_digit() => self.number(),
            Some(character) => Err(format!("unsupported value starting with {character:?}")),
            None => Err("missing value".to_string()),
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<(), String> {
        if self.rest().starts_with(keyword) {
            self.position += keyword.len();
            Ok(())
        } else {
            Err(format!("expected {keyword}"))
        }
    }

    fn string(&mut self) -> Result<(), String> {
        self.expect('"')?;
        loop {
            match self.bump() {
                Some('"') => return Ok(()),
                Some('\\') => match self.bump() {
                    Some('\\' | '"' | 'n' | 'r' | 't' | '$') => {}
                    Some(character) => return Err(format!("unsupported escape \\{character}")),
                    None => return Err("unterminated escape".to_string()),
                },
                Some('$') if self.peek() == Some('{') => {
                    return Err("interpolation is not allowed".to_string());
                }
                Some(character) if character.is_control() => {
                    return Err("control character in string".to_string());
                }
                Some(_) => {}
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn list(&mut self) -> Result<(), String> {
        self.enter()?;
        self.expect('[')?;
        self.whitespace();
        while self.peek() != Some(']') {
            self.literal()?;
            self.whitespace();
        }
        self.expect(']')?;
        self.leave();
        Ok(())
    }

    fn attrs(&mut self) -> Result<(), String> {
        self.enter()?;
        self.expect('{')?;
        self.whitespace();
        while self.peek() != Some('}') {
            self.attribute_name()?;
            self.whitespace();
            self.expect('=')?;
            self.literal()?;
            self.whitespace();
            self.expect(';')?;
            self.whitespace();
        }
        self.expect('}')?;
        self.leave();
        Ok(())
    }

    fn attribute_name(&mut self) -> Result<(), String> {
        self.whitespace();
        if self.peek() == Some('"') {
            return self.string();
        }
        let start = self.position;
        if !matches!(self.peek(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        {
            return Err("invalid attribute name".to_string());
        }
        while matches!(
            self.peek(),
            Some(character)
                if character.is_ascii_alphanumeric()
                    || matches!(character, '_' | '\'' | '-')
        ) {
            self.position += 1;
        }
        if self.position == start {
            Err("invalid attribute name".to_string())
        } else {
            Ok(())
        }
    }

    fn package(&mut self) -> Result<(), String> {
        if !self.rest().starts_with("pkgs.") {
            return Err("only pkgs.<attrpath> package expressions are allowed".to_string());
        }
        self.position += "pkgs.".len();
        loop {
            let start = self.position;
            if !matches!(self.peek(), Some(character) if character.is_ascii_alphabetic() || character == '_')
            {
                return Err("invalid package attribute".to_string());
            }
            while matches!(
                self.peek(),
                Some(character)
                    if character.is_ascii_alphanumeric()
                        || matches!(character, '_' | '\'' | '-')
            ) {
                self.position += 1;
            }
            if self.position == start {
                return Err("invalid package attribute".to_string());
            }
            if self.peek() != Some('.') {
                return Ok(());
            }
            self.position += 1;
        }
    }

    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some('-') {
            self.position += 1;
        }
        let digits = self.digits()?;
        if digits == 0 {
            return Err("invalid number".to_string());
        }
        if self.peek() == Some('.') {
            self.position += 1;
            if self.digits()? == 0 {
                return Err("invalid number".to_string());
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.position += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            if self.digits()? == 0 {
                return Err("invalid number".to_string());
            }
        }
        Ok(())
    }

    fn digits(&mut self) -> Result<usize, String> {
        let start = self.position;
        while matches!(self.peek(), Some(character) if character.is_ascii_digit()) {
            self.position += 1;
        }
        Ok(self.position - start)
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|segment| {
            let mut characters = segment.chars();
            matches!(characters.next(), Some(character) if character.is_ascii_alphabetic() || character == '_')
                && characters.all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        })
}

fn safe_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 64
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn safe_id(id: &str) -> bool {
    id.len() == 32
        && id
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
}

fn plan_directory(uid: u32) -> PathBuf {
    if uid == 0 {
        PathBuf::from("/run/nyet/plans/0")
    } else {
        PathBuf::from(format!("/run/user/{uid}/nyet/plans"))
    }
}

fn wrapper(host: &str) -> String {
    format!(
        "{{\n  inputs.user.url = \"path:{CONFIGURATION_PATH}\";\n  outputs = {{ self, user }}: {{\n    nixosConfigurations.\"{host}\" = user.nixosConfigurations.\"{host}\".extendModules {{\n      modules = [ ./nyet.nix ];\n    }};\n  }};\n}}\n"
    )
}

fn rebuild(host: &str) -> Result<(), String> {
    let binary = rebuild_binary()?;
    let flake = format!("{STATE_DIR}#{host}");
    let status = Command::new(binary)
        .args(["switch", "--flake", &flake])
        .env_clear()
        .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin")
        .env("HOME", "/root")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .status()
        .map_err(|error| format!("cannot run nixos-rebuild: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("nixos-rebuild switch failed: {status}"))
    }
}

fn rebuild_rollback() -> Result<(), String> {
    let binary = rebuild_binary()?;
    let status = Command::new(binary)
        .args(["switch", "--rollback"])
        .env_clear()
        .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin")
        .env("HOME", "/root")
        .env("USER", "root")
        .env("LOGNAME", "root")
        .status()
        .map_err(|error| format!("cannot run nixos-rebuild: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("nixos-rebuild rollback failed: {status}"))
    }
}

fn rebuild_binary() -> Result<PathBuf, String> {
    [
        "/run/current-system/sw/bin/nixos-rebuild",
        "/nix/var/nix/profiles/system/sw/bin/nixos-rebuild",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|candidate| candidate.exists())
    .ok_or_else(|| "nixos-rebuild is not installed".to_string())
}

fn generation() -> Option<String> {
    fs::read_link("/run/current-system")
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn read_optional(path: &str) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path}: {error}")),
    }
}

fn restore(path: &str, contents: Option<&str>) -> Result<(), String> {
    match contents {
        Some(contents) => write_atomic(Path::new(path), contents.as_bytes(), 0o644),
        None => {
            let _ = fs::remove_file(path);
            Ok(())
        }
    }
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "nyet".to_string()),
        std::process::id()
    ));

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .open(&temporary)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        file.write_all(contents)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync {}: {error}", temporary.display()))?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = mode;
        fs::write(&temporary, contents)
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    }

    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))
}

fn write_transaction(transaction: &Transaction) -> Result<(), String> {
    let directory = Path::new(TRANSACTION_DIR);
    fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot set permissions on {}: {error}", directory.display()))?;
    let path = directory.join(format!("{}.json", transaction.id));
    let bytes = serde_json::to_vec_pretty(transaction)
        .map_err(|error| format!("cannot serialize transaction: {error}"))?;
    write_atomic(&path, &bytes, 0o644)
}

fn transaction_id(now: u64) -> String {
    format!("{now}-{}", random_hex(8))
}

fn random_hex(length: usize) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut bytes = [0u8; 16];
    let filled = fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .is_ok();
    let seed = if filled {
        u64::from_le_bytes(bytes[..8].try_into().expect("slice length"))
    } else {
        unix_time() ^ (std::process::id() as u64) << 32
    };
    let mut state = seed;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            DIGITS[(state % 16) as usize] as char
        })
        .collect()
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn caller_uid() -> Result<u32, String> {
    if let Ok(value) = std::env::var("PKEXEC_UID") {
        return value.parse().map_err(|_| "invalid PKEXEC_UID".to_string());
    }
    if effective_uid() == 0 {
        Ok(0)
    } else {
        Err("cannot determine the authenticated user".to_string())
    }
}

fn effective_uid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self")
            .map(|metadata| metadata.uid())
            .unwrap_or(u32::MAX)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

fn ensure_root() -> Result<(), String> {
    if effective_uid() == 0 {
        Ok(())
    } else {
        Err("nyet-helper must run as root".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "{ config, lib, pkgs, ... }:\n";

    fn module(body: &str) -> String {
        format!("{HEADER}{{\n{body}}}\n")
    }

    #[test]
    fn accepts_rendered_scalars_lists_and_attrsets() {
        for body in [
            "  services.openssh.enable = true;\n",
            "  services.xserver.videoDrivers = [ \"modesetting\" \"nvidia\" ];\n",
            "  users.users.alice = { name = \"alice\"; uid = 1000; };\n",
            "  environment.systemPackages = [ pkgs.hello pkgs.git ];\n",
            "  services.foo.ratio = -1.5e3;\n",
            "  services.foo.nothing = null;\n",
        ] {
            assert!(validate_module(&module(body)).is_ok(), "{body}");
        }
    }

    #[test]
    fn rejects_executable_nix() {
        for body in [
            "  services.x.enable = import ./x.nix;\n",
            "  services.x.enable = builtins.readFile ./secret;\n",
            "  services.x.enable = \"${builtins.readFile ./secret}\";\n",
            "  services.x = { y = 1; } // { z = 2; };\n",
        ] {
            assert!(validate_module(&module(body)).is_err(), "{body}");
        }
    }

    #[test]
    fn rejects_unsafe_paths_and_headers() {
        assert!(validate_module("{ ... }:\n{\n  services.x.enable = true;\n}\n").is_err());
        assert!(validate_module(&module("  services.x.\"enable\" = true;\n")).is_err());
        assert!(
            validate_module(&module("  services.x.enable = pkgs.hello.override { };\n")).is_err()
        );
        assert!(validate_module(&module("  services.x.enable = 1;\n}\n")).is_err());
        assert!(validate_module(&module(&format!(
            "  services.x.value = {};\n",
            "[".repeat(100) + &"]".repeat(100)
        )))
        .is_err());
    }

    #[test]
    fn wrapper_uses_fixed_paths_and_quoted_host() {
        let wrapper = wrapper("my-host");
        assert!(wrapper.contains("path:/etc/nixos"));
        assert!(wrapper.contains("nixosConfigurations.\"my-host\""));
        assert!(wrapper.contains("modules = [ ./nyet.nix ]"));
    }

    #[test]
    fn ids_and_hosts_are_restricted() {
        assert!(safe_id("0123456789abcdef0123456789abcdef"));
        assert!(!safe_id("../../etc/passwd"));
        assert!(safe_host("my-host_1"));
        assert!(!safe_host("host.with.dot"));
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nyet-helper-{}-{}-{name}",
            std::process::id(),
            unix_time()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn plan_file(
        directory: &Path,
        id: &str,
        uid: u32,
        created: u64,
        expires: u64,
        module: &str,
    ) -> PathBuf {
        let path = directory.join(format!("{id}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": PLAN_VERSION,
            "id": id,
            "uid": uid,
            "host": "machine",
            "created": created,
            "expires": expires,
            "module": module,
        }))
        .unwrap();
        write_atomic(&path, &bytes, 0o600).unwrap();
        path
    }

    #[test]
    fn reads_a_valid_plan_and_rejects_bad_metadata() {
        let directory = temporary_directory("plan");
        let uid = effective_uid();
        let id = "0123456789abcdef0123456789abcdef";
        let module = module("  services.openssh.enable = true;\n");
        let now = unix_time();
        let path = plan_file(&directory, id, uid, now, now + 600, &module);

        assert!(read_plan(&path, id, uid).is_ok());
        assert!(read_plan(&path, "fedcba9876543210fedcba9876543210", uid).is_err());

        let expired = plan_file(&directory, id, uid, now - 1000, now - 1, &module);
        assert!(read_plan(&expired, id, uid).is_err());

        let long = plan_file(&directory, id, uid, now, now + 601, &module);
        assert!(read_plan(&long, id, uid).is_err());

        let wrong_user = plan_file(&directory, id, uid + 1, now, now + 600, &module);
        assert!(read_plan(&wrong_user, id, uid).is_err());

        let invalid = plan_file(&directory, id, uid, now, now + 600, "not a module");
        assert!(read_plan(&invalid, id, uid).is_err());

        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_a_broad_plan_mode() {
        let directory = temporary_directory("plan-mode");
        let uid = effective_uid();
        let id = "0123456789abcdef0123456789abcdef";
        let module = module("  services.openssh.enable = true;\n");
        let now = unix_time();
        let path = directory.join(format!("{id}.json"));
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": PLAN_VERSION,
            "id": id,
            "uid": uid,
            "host": "machine",
            "created": now,
            "expires": now + 600,
            "module": module,
        }))
        .unwrap();
        write_atomic(&path, &bytes, 0o644).unwrap();

        assert!(read_plan(&path, id, uid).is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn plan_directories_are_uid_scoped() {
        assert_eq!(plan_directory(0), PathBuf::from("/run/nyet/plans/0"));
        assert_eq!(
            plan_directory(1000),
            PathBuf::from("/run/user/1000/nyet/plans")
        );
    }

    #[test]
    fn writes_atomically_and_restores() {
        let directory = temporary_directory("atomic");
        let path = directory.join("module.nix");
        write_atomic(&path, b"one", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        write_atomic(&path, b"two", 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let _ = fs::remove_dir_all(directory);
    }
}
