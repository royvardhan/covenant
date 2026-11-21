mod add;
mod check;
mod hook;
mod mutate;
mod prove;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use covenant_engine::{Config, DEFAULT_TIMEOUT_SECS, RunOptions};

#[derive(Parser)]
#[command(
    name = "covenant",
    version,
    about = "Run promise checks on every push; block the push if a promise breaks"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "covenant.toml",
        help = "Path to the promises file"
    )]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "Run the promises affected by the change (or all with --all)")]
    Check(check::Args),
    #[command(about = "List promises in covenant.toml")]
    List,
    #[command(about = "Manage the git pre-push hook")]
    Hook {
        #[command(subcommand)]
        cmd: hook::Cmd,
    },
    #[command(
        about = "Copy a pack's promises into covenant.toml (built-in name or path to a pack.toml)"
    )]
    Add { pack: String },
    #[command(about = "List the built-in packs")]
    Packs,
    #[command(about = "Have an AI agent write the check for a promise, then verify it")]
    Prove(prove::Args),
    #[command(about = "Break the code a promise protects and see whether its check notices")]
    Mutate(mutate::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("covenant: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.cmd {
        Cmd::Check(args) => check::run(&cli.config, args),
        Cmd::List => list(&cli.config),
        Cmd::Hook { cmd } => hook::run(&cli.config, cmd),
        Cmd::Add { pack } => add::run(&cli.config, &pack),
        Cmd::Packs => add::list(),
        Cmd::Prove(args) => prove::run(&cli.config, args),
        Cmd::Mutate(args) => mutate::run(&cli.config, args),
    }
}

/// The directory checks run from: where the config file lives.
fn config_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where and for how long checks run: the command line's timeout, else the
/// config's, else the default. A promise's own `timeout` still takes precedence
/// over all of these; the engine applies it per check.
fn run_options(config_path: &Path, cfg: &Config, flag: Option<u64>) -> RunOptions {
    let secs = flag.or(cfg.timeout).unwrap_or(DEFAULT_TIMEOUT_SECS);
    RunOptions {
        cwd: config_dir(config_path),
        timeout: Duration::from_secs(secs),
    }
}

/// Position of promise `id` in the config, or an error listing what exists.
fn promise_index(cfg: &Config, id: &str, config_path: &Path) -> Result<usize> {
    cfg.promises
        .iter()
        .position(|p| p.id == id)
        .with_context(|| {
            let ids: Vec<&str> = cfg.promises.iter().map(|p| p.id.as_str()).collect();
            format!(
                "no promise `{id}` in {}; promises: {}",
                config_path.display(),
                ids.join(", ")
            )
        })
}

/// Checks run in their own process groups, so the terminal's Ctrl-C does not
/// reach them. Take them down with us, and never leave a mutant on disk.
fn install_interrupt_handler() -> Result<()> {
    ctrlc::set_handler(|| {
        covenant_engine::run::abort_running();
        mutate::restore_and_freeze();
        std::process::exit(130);
    })
    .context("could not install the interrupt handler")
}

fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        bail!(
            "{} not found. Run `covenant add billing` to start from a pack, or create one with a [[promise]] table",
            path.display()
        );
    }
    Config::load(path).with_context(|| format!("could not load {}", path.display()))
}

fn list(config: &Path) -> Result<ExitCode> {
    let cfg = load_config(config)?;
    if cfg.promises.is_empty() {
        println!("no promises in {}", config.display());
        return Ok(ExitCode::SUCCESS);
    }
    let width = cfg.promises.iter().map(|p| p.id.len()).max().unwrap_or(0);
    for p in &cfg.promises {
        let pack = p
            .pack
            .as_deref()
            .map(|s| format!("  [{s}]"))
            .unwrap_or_default();
        println!("{:<width$}  {}{}", p.id, p.statement, pack);
        println!("{:<width$}  paths: {}", "", p.paths.join(", "));
        println!("{:<width$}  check: {}", "", p.check);
    }
    Ok(ExitCode::SUCCESS)
}
