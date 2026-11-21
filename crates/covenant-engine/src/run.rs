use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::config::Promise;
use crate::report::{Outcome, PromiseResult};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub cwd: PathBuf,
    pub timeout: Duration,
}

pub fn run_all<F>(promises: &[&Promise], opts: &RunOptions, on_done: F) -> Vec<PromiseResult>
where
    F: Fn(&PromiseResult) + Sync,
{
    let mut results: Vec<(usize, PromiseResult)> = promises
        .par_iter()
        .enumerate()
        .map(|(i, p)| {
            let r = run_one(p, opts);
            on_done(&r);
            (i, r)
        })
        .collect();
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, r)| r).collect()
}

pub fn run_one(promise: &Promise, opts: &RunOptions) -> PromiseResult {
    let timeout = promise
        .timeout
        .map(Duration::from_secs)
        .unwrap_or(opts.timeout);
    let started = Instant::now();
    let mut result = PromiseResult {
        id: promise.id.clone(),
        statement: promise.statement.clone(),
        pack: promise.pack.clone(),
        check: promise.check.clone(),
        outcome: Outcome::Block,
        exit_code: None,
        duration_ms: 0,
        output: String::new(),
    };

    let mut child = match shell(&promise.check)
        .current_dir(&opts.cwd)
        .env("COVENANT_PROMISE_ID", &promise.id)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            result.output = format!("failed to spawn check: {e}");
            result.duration_ms = elapsed_ms(started);
            return result;
        }
    };

    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let status = wait_with_timeout(&mut child, timeout);
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    result.duration_ms = elapsed_ms(started);
    result.output = merge(out, err);

    match status {
        Some(status) => {
            result.exit_code = status.code();
            result.outcome = if status.success() {
                Outcome::Pass
            } else {
                Outcome::Block
            };
        }
        None => {
            result.outcome = Outcome::Timeout;
            result.output.push_str(&format!(
                "\n[covenant] check exceeded timeout of {}s and was killed",
                timeout.as_secs()
            ));
        }
    }
    result
}

fn shell(check: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(check);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(check);
        own_process_group(&mut c);
        c
    }
}

/// Put the check in its own process group so a timeout can kill everything it
/// started, not just the shell. A surviving grandchild would keep the output
/// pipes open and hold `run_one` until it exited on its own.
#[cfg(unix)]
fn own_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    // The child leads its own group (see `own_process_group`), so pgid == pid.
    // SAFETY: killpg has no memory-safety preconditions. The child has not been
    // reaped yet, so its pid cannot have been reused.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_tree(child: &mut Child) {
    let _ = child.kill();
}

fn drain<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut r) = reader {
            let _ = r.read_to_string(&mut buf);
        }
        buf
    })
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() >= deadline => {
                kill_tree(child);
                let _ = child.wait();
                return None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => return child.wait().ok(),
        }
    }
}

fn merge(out: String, err: String) -> String {
    match (out.trim().is_empty(), err.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => out,
        (true, false) => err,
        (false, false) => format!("{out}\n{err}"),
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn promise(check: &str, timeout: Option<u64>) -> Promise {
        Promise {
            id: "T".into(),
            statement: "t".into(),
            why: None,
            paths: vec!["**".into()],
            check: check.into(),
            pack: None,
            timeout,
        }
    }

    fn opts() -> RunOptions {
        RunOptions {
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn exit_zero_passes() {
        let r = run_one(&promise("echo hi", None), &opts());
        assert_eq!(r.outcome, Outcome::Pass);
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.output.trim(), "hi");
    }

    #[test]
    fn exit_nonzero_blocks_and_keeps_stderr() {
        let r = run_one(&promise("echo boom >&2; exit 3", None), &opts());
        assert_eq!(r.outcome, Outcome::Block);
        assert_eq!(r.exit_code, Some(3));
        assert!(r.output.contains("boom"));
    }

    #[test]
    fn timeout_kills_and_reports() {
        let r = run_one(&promise("sleep 5", Some(1)), &opts());
        assert_eq!(r.outcome, Outcome::Timeout);
        assert!(r.duration_ms < 4000);
    }

    #[test]
    fn timeout_kills_processes_the_shell_forked() {
        // `; true` stops the shell from exec'ing `sleep` in place, so `sleep`
        // runs as a grandchild on every platform.
        let r = run_one(&promise("sleep 5; true", Some(1)), &opts());
        assert_eq!(r.outcome, Outcome::Timeout);
        assert!(r.duration_ms < 4000, "took {}ms", r.duration_ms);
    }

    #[test]
    fn run_all_preserves_order() {
        let ps = vec![promise("sleep 0.2; exit 1", None), promise("true", None)];
        let refs: Vec<&Promise> = ps.iter().collect();
        let rs = run_all(&refs, &opts(), |_| {});
        assert_eq!(rs[0].outcome, Outcome::Block);
        assert_eq!(rs[1].outcome, Outcome::Pass);
    }
}
