use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use covenant_engine::{Outcome, Reason, Report, git, run, select};

use crate::load_config;
use crate::ui;

#[derive(ClapArgs)]
pub struct Args {
    #[arg(
        long,
        help = "Git ref to diff against (default: upstream, origin/HEAD, main)"
    )]
    base: Option<String>,
    #[arg(long, default_value = "HEAD", help = "Git ref being checked")]
    head: String,
    #[arg(long, help = "Run every promise regardless of what changed")]
    all: bool,
    #[arg(long, help = "Per-check timeout in seconds (overrides covenant.toml)")]
    timeout: Option<u64>,
    #[arg(
        long,
        default_value = "covenant-report.json",
        help = "Where to write the JSON report"
    )]
    report: PathBuf,
    #[arg(long, help = "Only print verdicts, not check output")]
    quiet: bool,
}

pub fn run(config_path: &Path, args: Args) -> Result<ExitCode> {
    let cfg = load_config(config_path)?;
    let cwd = crate::config_dir(config_path);

    let (selection, base, head, changed) = if args.all {
        (select::select_all(&cfg.promises), None, None, Vec::new())
    } else {
        let base = match args.base.clone() {
            Some(b) => b,
            None => git::default_base(&cwd)
                .context("could not pick a base ref: pass --base <ref> or use --all")?,
        };
        let changed = git::changed_files(&cwd, &base, &args.head)
            .with_context(|| format!("diffing {base}...{}", args.head))?;
        let selection = select::select(&cfg.promises, &changed)?;
        (selection, Some(base), Some(args.head.clone()), changed)
    };

    ui::header(&selection, base.as_deref(), &changed, cfg.promises.len());

    if selection.promises.is_empty() {
        let report = Report {
            generated_at_unix: Report::now(),
            base,
            head,
            reason: reason_str(&selection.reason),
            changed_files: changed,
            total_promises: cfg.promises.len(),
            results: Vec::new(),
        };
        report.write(&args.report)?;
        ui::footer(&report, &args.report);
        return Ok(ExitCode::SUCCESS);
    }

    let opts = crate::run_options(config_path, &cfg, args.timeout);
    let quiet = args.quiet;
    crate::install_interrupt_handler()?;
    let results = run::run_all(&selection.promises, &opts, |r| ui::verdict(r, quiet));

    let report = Report {
        generated_at_unix: Report::now(),
        base,
        head,
        reason: reason_str(&selection.reason),
        changed_files: changed,
        total_promises: cfg.promises.len(),
        results,
    };
    report.write(&args.report)?;
    ui::footer(&report, &args.report);

    if report.blocked() {
        if report.count(Outcome::Timeout) > 0 && report.count(Outcome::Block) == 0 {
            bail!("a check timed out; raise --timeout or the promise's `timeout`");
        }
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn reason_str(r: &Reason) -> String {
    match r {
        Reason::All => "all".into(),
        Reason::Trigger(f) => format!("trigger:{f}"),
        Reason::Paths => "paths".into(),
        Reason::NoChanges => "no-changes".into(),
    }
}
