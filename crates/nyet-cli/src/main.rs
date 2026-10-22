use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

use clap::{Parser, Subcommand};
use nyet_core::{
    apply_safe_host, apply_safe_path, build_plan, current_uid, default_cache_dir,
    default_state_dir, migration_plan, scan, stage_plan, validate, Engine, EngineConfig, Error,
    Intent, IntentOp, IntentStore, OptionPath, OptionSpec, Plan, Result, Value,
};

const HELPER_PATH: &str = "/run/current-system/sw/bin/nyet-helper";
const CONFIGURATION_PATH: &str = "/etc/nixos";

#[derive(Debug, Parser)]
#[command(
    name = "nyet",
    version,
    about = "Inspect and plan declarative NixOS configuration changes"
)]
struct Cli {
    #[arg(long, global = true)]
    flake: Option<PathBuf>,
    #[arg(long, global = true)]
    host: Option<String>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    no_cache: bool,
    #[arg(long, global = true)]
    refresh: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Options {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        include_internal: bool,
    },
    Get {
        path: String,
    },
    Set {
        path: String,
        value: String,
    },
    Unset {
        path: String,
    },
    Add {
        path: String,
        value: String,
    },
    Remove {
        path: String,
        value: String,
    },
    Intents {
        #[arg(long)]
        clear: bool,
    },
    Plan {
        #[arg(long = "set", value_name = "PATH=VALUE")]
        set: Vec<String>,
        #[arg(long)]
        base: Option<PathBuf>,
        #[arg(long)]
        check: bool,
    },
    Apply {
        #[arg(long)]
        yes: bool,
    },
    Rollback {
        #[arg(long)]
        yes: bool,
    },
    History,
    Scan,
    MigrationPlan {
        #[arg(long)]
        system: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nyet: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Command::Options {
            query,
            prefix,
            include_internal,
        } => options(&cli, query.clone(), prefix.clone(), *include_internal),
        Command::Get { path } => get(&cli, path),
        Command::Set { path, value } => set(&cli, path, value),
        Command::Unset { path } => unset(&cli, path),
        Command::Add { path, value } => add(&cli, path, value),
        Command::Remove { path, value } => remove(&cli, path, value),
        Command::Intents { clear } => intents(&cli, *clear),
        Command::Plan { set, base, check } => plan(&cli, set, base.as_deref(), *check),
        Command::Apply { yes } => apply(&cli, *yes),
        Command::Rollback { yes } => rollback(&cli, *yes),
        Command::History => history(&cli),
        Command::Scan => scan_repo(&cli),
        Command::MigrationPlan { system } => migration_plan_for(&cli, system),
    }
}

fn options(
    cli: &Cli,
    query: Option<String>,
    prefix: Option<String>,
    include_internal: bool,
) -> Result<()> {
    let engine = engine_for(cli)?;
    let query = query.map(|value| value.to_ascii_lowercase());

    let mut options: Vec<OptionSpec> = specs(&engine, cli.refresh)?
        .into_iter()
        .filter(|option| include_internal || !option.internal)
        .filter(|option| {
            query
                .as_ref()
                .is_none_or(|query| option.path.as_str().to_ascii_lowercase().contains(query))
        })
        .filter(|option| {
            prefix
                .as_ref()
                .is_none_or(|prefix| matches_prefix(option.path.as_str(), prefix))
        })
        .collect();

    options.sort_by(|left, right| left.path.cmp(&right.path));

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&options)?));
    }

    let mut text = String::new();
    for option in options {
        let default = option
            .default_text
            .clone()
            .or_else(|| option.default.as_ref().map(Value::to_nix))
            .unwrap_or_default();
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            option.path,
            option.type_name,
            default,
            summarize(&option.description)
        ));
    }

    emit(&text)
}

fn get(cli: &Cli, path: &str) -> Result<()> {
    let engine = engine_for(cli)?;
    let path = OptionPath::parse(path)?;
    let view = engine.get(&path)?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&view)?));
    }

    let spec = view
        .spec
        .as_ref()
        .ok_or_else(|| Error::UnknownOption(path.to_string()))?;
    let value = view
        .value
        .as_ref()
        .map(Value::to_nix)
        .unwrap_or_else(|| "-".to_string());
    let default = spec
        .default
        .as_ref()
        .map(Value::to_nix)
        .or_else(|| spec.default_text.clone())
        .unwrap_or_else(|| "-".to_string());

    let mut text = String::new();
    text.push_str(&format!("path: {path}\n"));
    text.push_str(&format!("type: {}\n", spec.type_name));
    text.push_str(&format!("value: {value}\n"));
    text.push_str(&format!("default: {default}\n"));
    text.push_str(&format!("read only: {}\n", spec.read_only));
    text.push_str(&format!("internal: {}\n", spec.internal));
    text.push_str(&format!("defined: {}\n", spec.defined));
    text.push_str("description:\n");
    text.push_str(spec.description.trim_end());
    text.push('\n');
    if !spec.declarations.is_empty() {
        text.push_str("declarations:\n");
        for declaration in &spec.declarations {
            text.push_str(&format!("  - {declaration}\n"));
        }
    }
    if !spec.definition_files.is_empty() {
        text.push_str("definitions:\n");
        for definition in &spec.definition_files {
            text.push_str(&format!("  - {definition}\n"));
        }
    }

    emit(&text)
}

fn set(cli: &Cli, path: &str, value: &str) -> Result<()> {
    let engine = engine_for(cli)?;
    let mut options = specs(&engine, cli.refresh)?;
    let path = OptionPath::parse(path)?;
    let spec = find_option(&options, &path)?;
    let value = spec.coerce_value(Value::parse_cli(value));
    let intent = Intent::set(path.clone(), value.clone());
    add_provenance(&engine, &mut options, std::slice::from_ref(&intent))?;
    validate(std::slice::from_ref(&intent), &options)?;

    store_for(cli, &engine)?.set(intent.clone())?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&intent)?));
    }
    emit(&format!("set {path} = {}\n", value.to_nix()))
}

fn unset(cli: &Cli, path: &str) -> Result<()> {
    let engine = engine_for(cli)?;
    let options = specs(&engine, cli.refresh)?;
    let path = OptionPath::parse(path)?;
    let spec = find_option(&options, &path)?;
    if spec.read_only {
        return Err(Error::InvalidValue(format!("{path} is read-only")));
    }
    let intent = Intent::reset(path.clone());

    store_for(cli, &engine)?.reset(path.clone())?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&intent)?));
    }
    emit(&format!("reset {path}\n"))
}

fn add(cli: &Cli, path: &str, value: &str) -> Result<()> {
    let engine = engine_for(cli)?;
    let options = specs(&engine, cli.refresh)?;
    let path = OptionPath::parse(path)?;
    let spec = find_option(&options, &path)?;
    let value = spec.coerce_element(Value::parse_cli(value));
    let intent = Intent::list_add(path.clone(), value.clone());
    validate(std::slice::from_ref(&intent), &options)?;

    store_for(cli, &engine)?.list_add(intent.clone())?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&intent)?));
    }
    emit(&format!("list add {path} = {}\n", value.to_nix()))
}

fn remove(cli: &Cli, path: &str, value: &str) -> Result<()> {
    let engine = engine_for(cli)?;
    let options = specs(&engine, cli.refresh)?;
    let path = OptionPath::parse(path)?;
    let spec = find_option(&options, &path)?;
    let value = spec.coerce_element(Value::parse_cli(value));
    validate(
        &[Intent::list_remove(path.clone(), value.clone())],
        &options,
    )?;

    store_for(cli, &engine)?.list_remove(path.clone(), value.clone())?;

    if cli.json {
        return emit(&format!(
            "{}\n",
            serde_json::to_string_pretty(&Intent::list_remove(path, value))?
        ));
    }
    emit(&format!("list remove {path} = {}\n", value.to_nix()))
}

fn intents(cli: &Cli, clear: bool) -> Result<()> {
    let engine = engine_for(cli)?;
    let mut store = store_for(cli, &engine)?;

    if clear {
        let count = store.clear()?;
        if cli.json {
            return emit(&format!(
                "{}\n",
                serde_json::to_string_pretty(&serde_json::json!({ "cleared": count }))?
            ));
        }
        return emit(&format!("cleared {count} intent(s)\n"));
    }

    let intents = store.intents();
    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(intents)?));
    }

    let mut text = String::new();
    for intent in intents {
        text.push_str(&format_intent(intent));
        text.push('\n');
    }
    emit(&text)
}

fn plan(cli: &Cli, set: &[String], base: Option<&Path>, check: bool) -> Result<()> {
    let engine = engine_for(cli)?;
    let plan = generate_plan(&engine, cli, set, base)?;

    if cli.json {
        let check = if check {
            Some(engine.check_module(&plan.module)?)
        } else {
            None
        };
        let mut output = serde_json::to_value(&plan)?;
        output["check"] = serde_json::json!(check);
        return emit(&format!("{}\n", serde_json::to_string_pretty(&output)?));
    }

    emit(&plan_text(&plan))?;

    if check {
        match engine.check_module(&plan.module) {
            Ok(drv) => emit(&format!("check: ok ({drv})\n"))?,
            Err(error) => {
                emit(&format!("check: failed: {error}\n"))?;
                return Err(error);
            }
        }
    }

    Ok(())
}

fn apply(cli: &Cli, yes: bool) -> Result<()> {
    if cli.json {
        return Err(Error::Usage(
            "apply does not support --json; use nyet history --json".to_string(),
        ));
    }

    let engine = engine_for(cli)?;
    ensure_apply_configuration(&engine)?;
    let host = engine.config().host.clone();
    if !apply_safe_host(&host) {
        return Err(Error::Usage(format!(
            "{host:?} is not a valid host for apply; use letters, digits, '_' or '-'"
        )));
    }

    let plan = generate_plan(&engine, cli, &[], None)?;
    for intent in &plan.intents {
        if !apply_safe_path(intent.path.as_str()) {
            return Err(Error::Usage(format!(
                "{} cannot be applied; apply supports unquoted option paths only",
                intent.path
            )));
        }
    }

    emit(&plan_text(&plan))?;
    let drv = engine.check_module(&plan.module)?;
    emit(&format!("check: ok ({drv})\n"))?;

    if !yes && !confirm(&format!("Apply this plan to {host}?"))? {
        return Err(Error::Usage("apply cancelled".to_string()));
    }

    let staged = stage_plan(&plan.module, &host)?;
    emit(&format!("applying plan {}\n", staged.id))?;
    let status = run_helper(&["apply", staged.id.as_str()])?;
    if !status.success() {
        return Err(Error::Usage(format!("nyet-helper apply failed: {status}")));
    }
    emit(&format!("applied plan {}\n", staged.id))
}

fn rollback(cli: &Cli, yes: bool) -> Result<()> {
    if cli.json {
        return Err(Error::Usage(
            "rollback does not support --json; use nyet history --json".to_string(),
        ));
    }
    if !yes && !confirm("Roll back one NixOS generation?")? {
        return Err(Error::Usage("rollback cancelled".to_string()));
    }

    let status = run_helper(&["rollback"])?;
    if !status.success() {
        return Err(Error::Usage(format!(
            "nyet-helper rollback failed: {status}"
        )));
    }
    emit("rollback complete\n")
}

fn scan_repo(cli: &Cli) -> Result<()> {
    let root = cli
        .flake
        .clone()
        .unwrap_or_else(|| PathBuf::from(CONFIGURATION_PATH));
    let repo = scan(&root, cli.host.as_deref())?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&repo)?));
    }

    let kind = match repo.kind {
        nyet_core::RepoKind::Flake => "flake",
        nyet_core::RepoKind::Classic => "classic",
    };
    let mut text = String::new();
    text.push_str(&format!("root: {}\n", repo.root.display()));
    text.push_str(&format!("kind: {kind}\n"));
    text.push_str(&format!(
        "host: {}\n",
        repo.host.as_deref().unwrap_or("unknown")
    ));
    text.push_str(&format!("standard: {}\n", repo.standard));
    text.push_str(&format!("files: {}\n", repo.files.len()));
    if !repo.missing.is_empty() {
        text.push_str(&format!("missing: {}\n", repo.missing.join(", ")));
    }
    for warning in &repo.warnings {
        text.push_str(&format!("warning: {warning}\n"));
    }
    for file in &repo.files {
        text.push_str(&format!("  {}\n", file.path));
        for import in &file.imports {
            text.push_str(&format!("    import {import}\n"));
        }
        for error in &file.syntax_errors {
            text.push_str(&format!("    syntax error: {error}\n"));
        }
    }
    emit(&text)
}

fn migration_plan_for(cli: &Cli, system: &str) -> Result<()> {
    let root = cli
        .flake
        .clone()
        .unwrap_or_else(|| PathBuf::from(CONFIGURATION_PATH));
    let repo = scan(&root, cli.host.as_deref())?;
    let host = cli
        .host
        .clone()
        .ok_or_else(|| Error::Usage("--host is required".to_string()))?;
    let plan = migration_plan(&repo, &host, system)?;

    if cli.json {
        return emit(&format!("{}\n", serde_json::to_string_pretty(&plan)?));
    }

    let mut text = String::new();
    text.push_str(&format!("root: {}\n", plan.root.display()));
    text.push_str(&format!("host: {}\n", plan.host));
    text.push_str(&format!("system: {}\n", plan.system));
    text.push_str(&format!(
        "changed files: {}\n",
        plan.changed_files().join(", ")
    ));
    for operation in &plan.operations {
        match operation {
            nyet_core::FileOperation::Create { path, contents } => {
                text.push_str(&format!("create {path}\n{contents}"));
            }
            nyet_core::FileOperation::Rename { from, to } => {
                text.push_str(&format!("rename {from} -> {to}\n"));
            }
        }
    }
    for warning in &plan.warnings {
        text.push_str(&format!("warning: {warning}\n"));
    }
    emit(&text)
}

fn history(cli: &Cli) -> Result<()> {
    let transactions = nyet_core::history()?;

    if cli.json {
        return emit(&format!(
            "{}\n",
            serde_json::to_string_pretty(&transactions)?
        ));
    }

    let mut text = String::new();
    for transaction in transactions {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{} -> {}\n",
            transaction.finished,
            transaction.kind,
            transaction.status,
            transaction.plan_id.unwrap_or_else(|| "-".to_string()),
            transaction.before.unwrap_or_else(|| "-".to_string()),
            transaction.after.unwrap_or_else(|| "-".to_string()),
        ));
        if let Some(message) = transaction.message {
            text.push_str(&format!("  {message}\n"));
        }
    }
    emit(&text)
}

fn generate_plan(engine: &Engine, cli: &Cli, set: &[String], base: Option<&Path>) -> Result<Plan> {
    let transient = set
        .iter()
        .map(|entry| parse_set(entry))
        .collect::<Result<Vec<_>>>()?;

    let mut options = specs(engine, cli.refresh)?;
    let mut intents = store_for(cli, engine)?.intents().to_vec();
    intents.extend(transient);
    if intents.is_empty() {
        return Err(Error::Usage(
            "no pending intents; use nyet set, nyet add, or plan --set".to_string(),
        ));
    }
    coerce_intents(&mut intents, &options);
    add_provenance(engine, &mut options, &intents)?;

    let base_text = match base {
        Some(path) => std::fs::read_to_string(path)?,
        None => String::new(),
    };
    let base_name = base
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "/dev/null".to_string());

    build_plan(&options, &intents, &base_text, &base_name)
}

fn plan_text(plan: &Plan) -> String {
    let mut text = plan.module.clone();
    text.push_str("--- diff ---\n");
    text.push_str(&plan.diff);
    for warning in &plan.warnings {
        text.push_str(&format!("warning: {warning}\n"));
    }
    text
}

fn ensure_apply_configuration(engine: &Engine) -> Result<()> {
    let configured = std::fs::canonicalize(CONFIGURATION_PATH).map_err(|_| {
        Error::Usage(format!(
            "apply requires a flake at {CONFIGURATION_PATH} in this MVP"
        ))
    })?;
    if configured != engine.config().flake {
        return Err(Error::Usage(format!(
            "apply only supports the flake at {CONFIGURATION_PATH}; configured flake is {}",
            engine.config().flake.display()
        )));
    }
    Ok(())
}

fn run_helper(arguments: &[&str]) -> Result<std::process::ExitStatus> {
    let helper = Path::new(HELPER_PATH);
    if !helper.exists() {
        return Err(Error::Usage(format!(
            "helper is not installed at {HELPER_PATH}; enable the nyet NixOS module"
        )));
    }

    let mut command = if current_uid()? == 0 {
        let mut command = ProcessCommand::new(helper);
        command.args(arguments);
        command
    } else {
        let pkexec = ["/run/current-system/sw/bin/pkexec", "/usr/bin/pkexec"]
            .into_iter()
            .map(Path::new)
            .find(|candidate| candidate.exists())
            .ok_or_else(|| Error::Usage("pkexec is not installed".to_string()))?;
        let mut command = ProcessCommand::new(pkexec);
        command.arg(helper).args(arguments);
        command
    };

    command.status().map_err(Error::from)
}

fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{IsTerminal, Write};

    if !std::io::stdin().is_terminal() {
        return Err(Error::Usage(
            "confirmation requires a terminal; use --yes".to_string(),
        ));
    }

    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn parse_set(entry: &str) -> Result<Intent> {
    let (path, value) = entry
        .split_once('=')
        .ok_or_else(|| Error::Usage(format!("--set expects PATH=VALUE, got {entry:?}")))?;
    Ok(Intent::set(
        OptionPath::parse(path)?,
        Value::parse_cli(value),
    ))
}

fn add_provenance(engine: &Engine, options: &mut [OptionSpec], intents: &[Intent]) -> Result<()> {
    let mut paths: Vec<&OptionPath> = intents.iter().map(|intent| &intent.path).collect();
    paths.sort();
    paths.dedup();

    for path in paths {
        let Ok(view) = engine.get(path) else {
            continue;
        };
        let Some(provenance) = view.spec else {
            continue;
        };
        if let Some(option) = options.iter_mut().find(|option| &option.path == path) {
            option.defined = provenance.defined;
            option.definition_files = provenance.definition_files;
        }
    }
    Ok(())
}

fn coerce_intents(intents: &mut [Intent], options: &[OptionSpec]) {
    for intent in intents {
        let Some(spec) = options.iter().find(|option| option.path == intent.path) else {
            continue;
        };
        let Some(value) = intent.value.take() else {
            continue;
        };
        intent.value = Some(match intent.op {
            IntentOp::Set => spec.coerce_value(value),
            IntentOp::ListAdd | IntentOp::ListRemove => spec.coerce_element(value),
            IntentOp::Reset => value,
        });
    }
}

fn find_option<'a>(options: &'a [OptionSpec], path: &OptionPath) -> Result<&'a OptionSpec> {
    options
        .iter()
        .find(|option| &option.path == path)
        .ok_or_else(|| Error::UnknownOption(path.to_string()))
}

fn format_intent(intent: &Intent) -> String {
    let value = intent.value.as_ref().map(Value::to_nix).unwrap_or_default();
    match intent.op {
        IntentOp::Set => format!("set {} = {value}", intent.path),
        IntentOp::Reset => format!("reset {}", intent.path),
        IntentOp::ListAdd => format!("list_add {} = {value}", intent.path),
        IntentOp::ListRemove => format!("list_remove {} = {value}", intent.path),
    }
}

fn specs(engine: &Engine, refresh: bool) -> Result<Vec<OptionSpec>> {
    if refresh {
        engine.refresh_options()
    } else {
        engine.options()
    }
}

fn engine_for(cli: &Cli) -> Result<Engine> {
    let flake = cli
        .flake
        .clone()
        .ok_or_else(|| Error::Usage("--flake is required".to_string()))?;
    let host = cli
        .host
        .clone()
        .ok_or_else(|| Error::Usage("--host is required".to_string()))?;
    let cache_dir = if cli.no_cache {
        None
    } else {
        default_cache_dir()
    };
    Engine::new(EngineConfig {
        flake,
        host,
        cache_dir,
    })
}

fn store_for(_cli: &Cli, engine: &Engine) -> Result<IntentStore> {
    let directory = default_state_dir().ok_or_else(|| {
        Error::Usage("cannot determine the nyet state directory; set NYET_STATE_DIR".to_string())
    })?;
    IntentStore::open(&directory, &engine.config().flake, &engine.config().host)
}

fn emit(text: &str) -> Result<()> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    match handle
        .write_all(text.as_bytes())
        .and_then(|()| handle.flush())
    {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn summarize(description: &str) -> String {
    let text = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 100 {
        let mut shortened = text.chars().take(97).collect::<String>();
        shortened.push_str("...");
        shortened
    } else {
        text
    }
}

fn matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}
