use std::io::IsTerminal;
use std::path::Path;
use std::sync::Mutex;

use covenant_engine::{Outcome, PromiseResult, Reason, Report, Selection};

static LOCK: Mutex<()> = Mutex::new(());

fn color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(code: &str, s: &str) -> String {
    if color() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn dim(s: &str) -> String {
    paint("2", s)
}

fn outcome(o: Outcome) -> String {
    match o {
        Outcome::Pass => paint("1;32", "PASS "),
        Outcome::Block => paint("1;31", "BLOCK"),
        Outcome::Timeout => paint("1;33", "TIMEOUT"),
    }
}

pub fn header(sel: &Selection<'_>, base: Option<&str>, changed: &[String], total: usize) {
    let n = sel.promises.len();
    let line = match &sel.reason {
        Reason::All => format!("running all {total} promises"),
        Reason::NoChanges => format!(
            "no changes since {}; nothing to check",
            base.unwrap_or("base")
        ),
        Reason::Trigger(f) => format!(
            "{} changed file(s) since {}; `{f}` changed, so running all {total} promises",
            changed.len(),
            base.unwrap_or("base")
        ),
        Reason::Paths => format!(
            "{} changed file(s) since {}; {n} of {total} promise(s) affected",
            changed.len(),
            base.unwrap_or("base")
        ),
    };
    println!("{} {line}", dim("covenant:"));
    if n > 0 {
        println!();
    }
}

pub fn verdict(r: &PromiseResult, quiet: bool) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let secs = r.duration_ms as f64 / 1000.0;
    let pack = r
        .pack
        .as_deref()
        .map(|p| dim(&format!(" [{p}]")))
        .unwrap_or_default();
    println!(
        "{}  {}  {}{}  {}",
        outcome(r.outcome),
        r.id,
        r.statement,
        pack,
        dim(&format!("({secs:.1}s)"))
    );
    if r.outcome.is_pass() || quiet {
        return;
    }
    println!("       {}", dim(&format!("$ {}", r.check)));
    for line in tail(&r.output, 40) {
        println!("       {line}");
    }
    println!();
}

/// One mutant's result. `good` colours the tag: caught, missed, or neither.
pub fn mutant(tag: &str, good: Option<bool>, label: &str) {
    let tag = match good {
        Some(true) => paint("1;32", tag),
        Some(false) => paint("1;31", tag),
        None => paint("1;33", tag),
    };
    println!("{tag}  {label}");
}

fn tail(s: &str, n: usize) -> Vec<&str> {
    let lines: Vec<&str> = s.lines().collect();
    let skip = lines.len().saturating_sub(n);
    let mut out = Vec::new();
    if skip > 0 {
        out.push("…");
    }
    out.extend(&lines[skip..]);
    out
}

pub fn footer(report: &Report, path: &Path) {
    if report.results.is_empty() {
        println!("{} report written to {}", dim("covenant:"), path.display());
        return;
    }
    let pass = report.count(Outcome::Pass);
    let block = report.count(Outcome::Block);
    let timeout = report.count(Outcome::Timeout);
    let mut parts = vec![format!("{pass} PASS")];
    if block > 0 {
        parts.push(format!("{block} BLOCK"));
    }
    if timeout > 0 {
        parts.push(format!("{timeout} TIMEOUT"));
    }
    println!();
    let summary = parts.join(", ");
    let summary = if report.blocked() {
        paint("1;31", &summary)
    } else {
        paint("1;32", &summary)
    };
    println!("{summary}  {}", dim(&format!("report: {}", path.display())));
    if report.blocked() {
        println!("{}", paint("1;31", "push blocked: a promise is broken"));
    }
}
