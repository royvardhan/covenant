use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use covenant_engine::mutate::{self, Site};
use covenant_engine::{DEFAULT_TIMEOUT_SECS, Outcome, Promise, RunOptions, git, run, select};

use crate::load_config;
use crate::ui;

const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_MUTANTS: usize = 10;
pub const DEFAULT_SEED: u64 = 1;

#[derive(ClapArgs)]
pub struct Args {
    #[arg(help = "Id of the promise whose check to test")]
    id: String,
    #[command(flatten)]
    opts: MutateArgs,
    #[arg(long, help = "Timeout in seconds for each run of the check")]
    timeout: Option<u64>,
}

#[derive(ClapArgs, Clone)]
pub struct MutateArgs {
    #[arg(long, default_value_t = DEFAULT_MUTANTS, help = "How many mutants to try")]
    pub mutants: usize,
    #[arg(long, default_value_t = DEFAULT_SEED, help = "Seed that picks which mutants are tried")]
    pub seed: u64,
    #[arg(
        long,
        help = "Command that exits 0 when the code still builds; mutants that fail it are discarded (default: `build` in covenant.toml)"
    )]
    pub build: Option<String>,
}

pub struct Summary {
    pub caught: usize,
    pub missed: Vec<Site>,
    pub timed_out: usize,
    pub invalid: usize,
}

impl Summary {
    pub fn judged(&self) -> usize {
        self.caught + self.missed.len()
    }
}

/// The file currently holding a mutant, and its real contents.
static PENDING: Mutex<Option<(PathBuf, Vec<u8>)>> = Mutex::new(None);

/// Put back the file a mutant was written to, if any, and stop any further
/// mutant from being written. Only for use right before the process exits.
pub fn restore_and_freeze() {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((path, original)) = pending.take() {
        let _ = fs::write(path, original);
    }
    std::mem::forget(pending);
}

/// Restores the mutated file when dropped, including on an early return.
struct Mutant;

impl Mutant {
    fn write(path: &Path, original: &[u8], mutated: &str) -> Result<Self> {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        *pending = Some((path.to_path_buf(), original.to_vec()));
        let guard = Mutant;
        fs::write(path, mutated).with_context(|| format!("writing {}", path.display()))?;
        Ok(guard)
    }
}

impl Drop for Mutant {
    fn drop(&mut self) {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((path, original)) = pending.take()
            && let Err(e) = fs::write(&path, original)
        {
            eprintln!(
                "covenant: COULD NOT RESTORE {} ({e}); run `git diff` and undo the mutation",
                path.display()
            );
        }
    }
}

pub fn run(config_path: &Path, args: Args) -> Result<ExitCode> {
    let cfg = load_config(config_path)?;
    let Some(promise) = cfg.get(&args.id).cloned() else {
        let ids: Vec<&str> = cfg.promises.iter().map(|p| p.id.as_str()).collect();
        bail!(
            "no promise `{}` in {}; promises: {}",
            args.id,
            config_path.display(),
            ids.join(", ")
        );
    };
    let cwd = crate::config_dir(config_path);
    let timeout = args
        .timeout
        .or(promise.timeout)
        .or(cfg.timeout)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let run_opts = RunOptions {
        cwd: cwd.clone(),
        timeout: Duration::from_secs(timeout),
    };
    let build = args.opts.build.clone().or(cfg.build.clone());

    let sites = candidate_sites(&cwd, &promise)?;
    if sites.is_empty() {
        bail!("{}", no_sites_message(&promise));
    }

    crate::install_interrupt_handler()?;
    let baseline = run::run_one(&promise, &run_opts);
    if !baseline.outcome.is_pass() {
        ui::verdict(&baseline, false);
        bail!(
            "{}'s check does not pass on the unmodified code; fix that before testing the check",
            promise.id
        );
    }

    let summary = run_mutants(&promise, sites, &args.opts, build.as_deref(), &run_opts)?;
    Ok(if report(&promise, &summary) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

pub fn no_sites_message(promise: &Promise) -> String {
    format!(
        "nothing to mutate for {}: its paths ({}) match no tracked source file with an operator \
         or boolean to swap. If the paths are a pack's defaults, point them at the real code",
        promise.id,
        promise.paths.join(", ")
    )
}

/// Every mutation site in the tracked source files the promise's paths match,
/// leaving out tests, manifests and prose.
pub fn candidate_sites(cwd: &Path, promise: &Promise) -> Result<Vec<Site>> {
    let globs = select::glob_set(promise)?;
    let tracked = git::tracked_files(cwd)
        .context("mutation needs a git repository to know which files are source")?;
    let mut sites = Vec::new();
    for file in tracked {
        if !globs.is_match(&file) || mutate::is_test_path(&file) || !mutate::is_code_path(&file) {
            continue;
        }
        let path = cwd.join(&file);
        if fs::metadata(&path).map_or(true, |m| !m.is_file() || m.len() > MAX_FILE_BYTES) {
            continue;
        }
        // Not UTF-8 means not source we can mutate.
        if let Ok(text) = fs::read_to_string(&path) {
            sites.extend(mutate::sites(&file, &text));
        }
    }
    Ok(sites)
}

/// Break the code one token at a time and see whether the promise's check
/// notices. The caller has already seen the check pass on the real code.
pub fn run_mutants(
    promise: &Promise,
    mut sites: Vec<Site>,
    args: &MutateArgs,
    build: Option<&str>,
    run_opts: &RunOptions,
) -> Result<Summary> {
    mutate::shuffle(&mut sites, args.seed);
    let total = sites.len();
    println!(
        "covenant: breaking the code {} way(s) to see whether {}'s check notices (seed {}, {} possible)",
        args.mutants.min(total),
        promise.id,
        args.seed,
        total
    );
    if build.is_none() {
        println!(
            "          no `build` command set, so a mutant that fails to compile also counts as caught"
        );
    }
    println!();

    let builder = build.map(|cmd| Promise {
        check: cmd.to_string(),
        ..promise.clone()
    });
    let mut summary = Summary {
        caught: 0,
        missed: Vec::new(),
        timed_out: 0,
        invalid: 0,
    };

    for site in sites {
        if summary.judged() + summary.timed_out >= args.mutants {
            break;
        }
        let path = run_opts.cwd.join(&site.file);
        let original = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let Some(mutated) = std::str::from_utf8(&original)
            .ok()
            .and_then(|text| mutate::apply(text, &site))
        else {
            continue;
        };
        let label = format!(
            "{}:{}  `{}` -> `{}`",
            site.file, site.line, site.from, site.to
        );

        let guard = Mutant::write(&path, &original, &mutated)?;
        if let Some(builder) = &builder
            && !run::run_one(builder, run_opts).outcome.is_pass()
        {
            drop(guard);
            summary.invalid += 1;
            ui::mutant("INVALID", None, &label);
            continue;
        }
        let result = run::run_one(promise, run_opts);
        drop(guard);

        match result.outcome {
            Outcome::Block => {
                summary.caught += 1;
                ui::mutant("CAUGHT ", Some(true), &label);
            }
            Outcome::Pass => {
                ui::mutant("MISSED ", Some(false), &label);
                summary.missed.push(site);
            }
            Outcome::Timeout => {
                summary.timed_out += 1;
                ui::mutant("TIMEOUT", None, &label);
            }
        }
    }
    println!();
    Ok(summary)
}

/// Print the verdict. True when the check caught at least one mutant.
pub fn report(promise: &Promise, s: &Summary) -> bool {
    let mut extra = Vec::new();
    if s.timed_out > 0 {
        extra.push(format!("{} timed out", s.timed_out));
    }
    if s.invalid > 0 {
        extra.push(format!("{} did not build", s.invalid));
    }
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(" ({} and are not counted)", extra.join(", "))
    };
    println!(
        "covenant: {}'s check caught {} of {} mutant(s){extra}",
        promise.id,
        s.caught,
        s.judged()
    );
    if s.caught == 0 {
        println!(
            "          The check passed on every broken version of the code it protects, so it has \
             not been shown to detect anything."
        );
        return false;
    }
    if !s.missed.is_empty() {
        println!(
            "          A missed mutant is a hole only if that line matters to this promise; \
             broad `paths` include code the promise does not cover."
        );
    }
    true
}
