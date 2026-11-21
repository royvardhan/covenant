use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::{ExitCode, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;
use covenant_engine::{Promise, git, prove, run, select};

use crate::load_config;
use crate::mutate::{self as mutants, MutateArgs};
use crate::ui;

#[derive(ClapArgs)]
pub struct Args {
    #[arg(help = "Id of the promise to write a check for")]
    id: String,
    #[arg(
        long,
        help = "Agent command; it gets the brief on stdin (default: $COVENANT_AGENT, then `agent` in covenant.toml)"
    )]
    agent: Option<String>,
    #[arg(long, help = "Print the brief the agent would get and exit")]
    print_prompt: bool,
    #[arg(
        long,
        help = "Accept the result even if the agent edited existing files the promise protects"
    )]
    allow_source_edits: bool,
    #[arg(long, help = "Timeout in seconds for running the new check")]
    timeout: Option<u64>,
    #[arg(
        long,
        help = "Record the check without first breaking the code to see whether it notices"
    )]
    no_mutate: bool,
    #[command(flatten)]
    mutate: MutateArgs,
}

/// Content fingerprint of every file that differs from HEAD, keyed by path.
/// The value is whether the file exists in HEAD and a hash of its bytes
/// (`None` when it cannot be read, e.g. it was deleted).
type Snapshot = BTreeMap<String, (bool, Option<u64>)>;

pub fn run(config_path: &Path, args: Args) -> Result<ExitCode> {
    let mut cfg = load_config(config_path)?;
    let index = crate::promise_index(&cfg, &args.id, config_path)?;
    let promise = cfg.promises[index].clone();
    let cwd = crate::config_dir(config_path);

    let out_name = format!(".covenant-prove-{}.check", file_safe(&promise.id));
    let brief = prove::prompt(&promise, &out_name);
    if args.print_prompt {
        print!("{brief}");
        return Ok(ExitCode::SUCCESS);
    }

    let agent = args
        .agent
        .or_else(|| {
            std::env::var("COVENANT_AGENT")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| cfg.agent.clone())
        .context(
            "no agent configured. Pass --agent '<command>', set COVENANT_AGENT, or add \
             `agent = \"<command>\"` at the top of covenant.toml. The command gets the brief \
             on stdin and must be able to edit files and run tests in this repository",
        )?;

    // Find out now, not after the agent has spent its time, whether there is
    // anything to break when it comes to testing the check.
    if !args.no_mutate && mutants::candidate_sites(&cwd, &promise)?.is_empty() {
        bail!(
            "{}. Or pass --no-mutate to record a check without that test",
            mutants::no_sites_message(&promise)
        );
    }

    let config_name = config_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let before = snapshot(&cwd)
        .context("`covenant prove` needs a git repository so it can show what the agent changed")?;

    let out_path = cwd.join(&out_name);
    let _ = fs::remove_file(&out_path);
    println!(
        "covenant: asking the agent to write the check for {}",
        promise.id
    );
    println!("          $ {agent}");
    println!();
    let status = run_agent(&agent, &cwd, &promise, &out_name, &brief);
    let proposed = fs::read_to_string(&out_path).ok();
    let _ = fs::remove_file(&out_path);
    let status = status?;
    println!();
    if !status.success() {
        bail!("the agent command failed ({status}); covenant.toml not updated");
    }

    let check = proposed.unwrap_or_default().trim().to_string();
    if check.is_empty() {
        bail!(
            "the agent did not write a check command to {out_name}; see its output above. \
             covenant.toml not updated"
        );
    }
    if check.lines().count() > 1 {
        bail!("the agent wrote more than one line to {out_name}; covenant.toml not updated");
    }
    if check.contains(prove::TODO_MARKER) {
        bail!("the agent's check still contains a TODO: `{check}`; covenant.toml not updated");
    }

    let after = snapshot(&cwd)?;
    let changed = changed_files(&before, &after, &[&out_name, &config_name]);
    let globs = select::glob_set(&promise)?;
    let protected: Vec<&str> = changed
        .iter()
        .filter(|(path, existing)| *existing && globs.is_match(path))
        .map(|(path, _)| path.as_str())
        .collect();

    if changed.is_empty() {
        println!("covenant: the agent changed no files");
    } else {
        println!("covenant: the agent changed {} file(s):", changed.len());
        for (path, existing) in &changed {
            let kind = if *existing { "edited" } else { "new   " };
            println!("          {kind}  {path}");
        }
    }
    println!();

    if !protected.is_empty() && !args.allow_source_edits {
        println!(
            "covenant: the agent edited existing code that {} protects:",
            promise.id
        );
        for path in &protected {
            println!("          {path}");
        }
        println!(
            "          A check must not be made to pass by changing the code it guards. Review \
             with `git diff`; if the edits are fine, rerun with --allow-source-edits."
        );
        println!("          covenant.toml not updated");
        return Ok(ExitCode::from(1));
    }

    let candidate = Promise {
        check: check.clone(),
        ..promise.clone()
    };
    let opts = crate::run_options(config_path, &cfg, args.timeout);
    crate::install_interrupt_handler()?;
    let result = run::run_one(&candidate, &opts);
    ui::verdict(&result, false);
    if !result.outcome.is_pass() {
        println!(
            "covenant: the new check does not pass, so covenant.toml was not updated. Either the \
             test is wrong or the code breaks the promise; the agent's summary above says which."
        );
        return Ok(ExitCode::from(1));
    }

    println!();
    if !args.no_mutate {
        let sites = mutants::candidate_sites(&cwd, &candidate)?;
        let build = args.mutate.build.clone().or(cfg.build.clone());
        let summary =
            mutants::run_mutants(&candidate, sites, &args.mutate, build.as_deref(), &opts)?;
        if !mutants::report(&candidate, &summary) {
            println!("          covenant.toml not updated");
            return Ok(ExitCode::from(1));
        }
        println!();
    }

    cfg.promises[index].check = check.clone();
    cfg.save(config_path)?;
    println!(
        "covenant: {} is now checked by `{check}` ({} updated)",
        promise.id,
        config_path.display()
    );
    println!("          Read the test and the agent's summary above, then commit.");
    Ok(ExitCode::SUCCESS)
}

fn run_agent(
    agent: &str,
    cwd: &Path,
    promise: &Promise,
    out_name: &str,
    brief: &str,
) -> Result<std::process::ExitStatus> {
    // Unlike a check, the agent stays in our process group: it is interactive
    // work the user is watching, and Ctrl-C should reach it directly.
    let mut child = run::shell_command(agent)
        .current_dir(cwd)
        .env("COVENANT_PROMISE_ID", &promise.id)
        .env("COVENANT_PROVE_OUT", out_name)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start the agent command `{agent}`"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // An agent that ignores stdin and exits early closes the pipe; that is
        // its business, and its exit status is what we report.
        let _ = stdin.write_all(brief.as_bytes());
    }
    child.wait().context("waiting for the agent command")
}

fn snapshot(cwd: &Path) -> Result<Snapshot> {
    Ok(git::dirty_files(cwd)?
        .into_iter()
        .map(|d| {
            let hash = fs::read(cwd.join(&d.path)).ok().map(|bytes| {
                let mut h = DefaultHasher::new();
                bytes.hash(&mut h);
                h.finish()
            });
            (d.path, (d.existing, hash))
        })
        .collect())
}

/// Files whose content differs between the two snapshots, with whether each
/// already existed in HEAD. A file the agent restored to its HEAD content
/// drops out of `after`; it existed, and it changed.
fn changed_files(before: &Snapshot, after: &Snapshot, ignore: &[&str]) -> Vec<(String, bool)> {
    let mut changed: BTreeMap<String, bool> = BTreeMap::new();
    for (path, (existing, hash)) in after {
        if before.get(path).map(|(_, h)| h) != Some(hash) {
            changed.insert(path.clone(), *existing);
        }
    }
    for (path, (existing, _)) in before {
        if !after.contains_key(path) {
            changed.insert(path.clone(), *existing);
        }
    }
    changed
        .into_iter()
        .filter(|(path, _)| !ignore.contains(&path.as_str()))
        .collect()
}

fn file_safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(entries: &[(&str, bool, Option<u64>)]) -> Snapshot {
        entries
            .iter()
            .map(|(p, e, h)| (p.to_string(), (*e, *h)))
            .collect()
    }

    #[test]
    fn changed_files_ignores_what_was_already_dirty_and_untouched() {
        let before = snap(&[("wip.rs", true, Some(1))]);
        let after = snap(&[("wip.rs", true, Some(1)), ("tests/new.rs", false, Some(2))]);
        assert_eq!(
            changed_files(&before, &after, &[]),
            vec![("tests/new.rs".to_string(), false)]
        );
    }

    #[test]
    fn changed_files_sees_further_edits_reverts_and_deletions() {
        let before = snap(&[("a.rs", true, Some(1)), ("b.rs", true, Some(1))]);
        let after = snap(&[("a.rs", true, Some(9)), ("c.rs", true, None)]);
        assert_eq!(
            changed_files(&before, &after, &[]),
            vec![
                ("a.rs".to_string(), true),
                ("b.rs".to_string(), true),
                ("c.rs".to_string(), true)
            ]
        );
    }

    #[test]
    fn changed_files_drops_ignored_names() {
        let after = snap(&[("covenant.toml", true, Some(1)), ("t.rs", false, Some(1))]);
        assert_eq!(
            changed_files(&Snapshot::new(), &after, &["covenant.toml"]),
            vec![("t.rs".to_string(), false)]
        );
    }
}
