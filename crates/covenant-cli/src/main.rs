mod add;
mod check;
mod hook;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use covenant_engine::Config;

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
    }
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
