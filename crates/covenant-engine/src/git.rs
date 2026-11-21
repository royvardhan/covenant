use std::path::Path;
use std::process::Command;

use crate::error::Error;

pub fn changed_files(dir: &Path, base: &str, head: &str) -> Result<Vec<String>, Error> {
    let out = git(
        dir,
        &[
            "diff",
            "--name-only",
            "--relative",
            &format!("{base}...{head}"),
        ],
    )?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

pub fn default_base(dir: &Path) -> Option<String> {
    [
        "@{upstream}",
        "origin/HEAD",
        "origin/main",
        "main",
        "master",
    ]
    .into_iter()
    .find(|r| {
        git(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{r}^{{commit}}"),
            ],
        )
        .is_ok()
    })
    .map(str::to_string)
}

/// A file that differs from HEAD or is untracked, relative to `dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirty {
    pub path: String,
    /// True when the file exists in HEAD, i.e. this is an edit, not a new file.
    pub existing: bool,
}

/// Every file under `dir` that differs from HEAD, staged or not, plus untracked
/// files that are not ignored.
pub fn dirty_files(dir: &Path) -> Result<Vec<Dirty>, Error> {
    let out = git(
        dir,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--",
            ".",
        ],
    )?;
    let prefix = git(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = prefix.trim();
    Ok(out
        .split('\0')
        .filter(|e| e.len() > 3)
        .map(|e| {
            let (status, path) = e.split_at(3);
            Dirty {
                path: path.strip_prefix(prefix).unwrap_or(path).to_string(),
                existing: !(status.starts_with("??") || status.starts_with('A')),
            }
        })
        .collect())
}

pub fn toplevel(dir: &Path) -> Result<String, Error> {
    git(dir, &["rev-parse", "--show-toplevel"]).map(|s| s.trim().to_string())
}

pub fn hooks_dir(dir: &Path) -> Result<String, Error> {
    git(dir, &["rev-parse", "--git-path", "hooks"]).map(|s| s.trim().to_string())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, Error> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Git(if stderr.is_empty() {
            format!("`git {}` failed", args.join(" "))
        } else {
            stderr
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
