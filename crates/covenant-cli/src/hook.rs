use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use covenant_engine::git;

const MARKER: &str = "# covenant pre-push hook";

const HOOK: &str = r#"#!/bin/sh
# covenant pre-push hook
# Installed by `covenant hook install`. Runs the promises affected by each ref being pushed.
zero=0000000000000000000000000000000000000000
status=0
while read -r local_ref local_sha remote_ref remote_sha; do
  [ "$local_sha" = "$zero" ] && continue
  if [ "$remote_sha" = "$zero" ]; then
    base=$(git merge-base "$local_sha" origin/HEAD 2>/dev/null || git merge-base "$local_sha" origin/main 2>/dev/null)
  else
    base=$remote_sha
  fi
  if [ -n "$base" ]; then
    covenant check --base "$base" --head "$local_sha" || status=1
  else
    covenant check --all || status=1
  fi
done
exit $status
"#;

#[derive(Subcommand)]
pub enum Cmd {
    #[command(about = "Install a git pre-push hook that runs `covenant check`")]
    Install {
        #[arg(
            long,
            help = "Overwrite an existing pre-push hook that covenant did not write"
        )]
        force: bool,
    },
    #[command(about = "Remove the pre-push hook installed by covenant")]
    Uninstall,
}

pub fn run(config_path: &Path, cmd: Cmd) -> Result<ExitCode> {
    let hook = hook_path(&crate::config_dir(config_path))?;
    match cmd {
        Cmd::Install { force } => install(&hook, force),
        Cmd::Uninstall => uninstall(&hook),
    }
}

fn hook_path(dir: &Path) -> Result<PathBuf> {
    let hooks = git::hooks_dir(dir).context("not inside a git repository")?;
    let hooks = PathBuf::from(hooks);
    let hooks = if hooks.is_absolute() {
        hooks
    } else {
        PathBuf::from(git::toplevel(dir)?).join(hooks)
    };
    Ok(hooks.join("pre-push"))
}

fn install(hook: &Path, force: bool) -> Result<ExitCode> {
    if hook.exists() {
        let existing = fs::read_to_string(hook).unwrap_or_default();
        if !existing.contains(MARKER) && !force {
            bail!(
                "{} already exists and was not written by covenant; re-run with --force to replace it",
                hook.display()
            );
        }
    }
    if let Some(parent) = hook.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(hook, HOOK).with_context(|| format!("writing {}", hook.display()))?;
    make_executable(hook)?;
    println!("installed {}", hook.display());
    println!(
        "every `git push` now runs `covenant check` against the remote ref and blocks on any broken promise"
    );
    Ok(ExitCode::SUCCESS)
}

fn uninstall(hook: &Path) -> Result<ExitCode> {
    if !hook.exists() {
        println!("no pre-push hook at {}", hook.display());
        return Ok(ExitCode::SUCCESS);
    }
    let existing = fs::read_to_string(hook).unwrap_or_default();
    if !existing.contains(MARKER) {
        bail!(
            "{} was not written by covenant; leaving it alone",
            hook.display()
        );
    }
    fs::remove_file(hook)?;
    println!("removed {}", hook.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}
