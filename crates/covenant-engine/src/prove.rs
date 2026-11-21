//! Building the brief an agent gets when asked to write the check for a promise.

use crate::config::Promise;

/// Marker packs put in a check template to say what the real check must prove.
pub const TODO_MARKER: &str = "# TODO";

/// The instructions handed to the agent on stdin. `out_file` is where the agent
/// must leave the command that runs the test it wrote.
pub fn prompt(promise: &Promise, out_file: &str) -> String {
    let why = promise
        .why
        .as_deref()
        .map(|w| format!("Why it matters: {w}\n"))
        .unwrap_or_default();
    let paths = promise.paths.join(", ");
    let (check, hint) = match promise.check.split_once(TODO_MARKER) {
        Some((cmd, hint)) => (
            cmd.trim(),
            format!(
                "Hint from the promise's author about what the check must prove: {}\n",
                hint.trim_start_matches(':').trim()
            ),
        ),
        None => (promise.check.trim(), String::new()),
    };
    let id = &promise.id;
    let statement = &promise.statement;

    format!(
        r#"You are working in a software repository that uses Covenant. Covenant keeps a
list of promises: statements about the system that must always stay true. Each
promise is enforced by a check, a shell command that exits 0 while the promise
holds and non-zero once it is broken. Covenant runs the check on every push and
blocks the push when it fails, so people will rely on this check to catch a real
regression months from now, long after anyone remembers how it was written.

Your task is to write the check for one promise.

Promise {id}
Statement: {statement}
{why}Code it protects (globs relative to the current directory): {paths}
Current check, a placeholder to replace: `{check}`
{hint}
What to do

1. Find how this repository implements the behaviour the statement describes.
   Start with the paths above, but they may be generic defaults that do not
   match this repository's layout, so look for the real code.
2. Write an automated test that fails if the statement stops being true. Use the
   language, test framework and conventions the repository already uses, and
   put the test where its tests live. The statement is universal ("never",
   "always"), so prefer a test that drives many inputs, for example a
   property-based or seeded randomised test, over a single example. Keep it
   deterministic and fast enough to run on every push.
3. Run the test and confirm it passes against the current code. Then confirm it
   can fail: temporarily break the behaviour, watch the test catch it, and
   restore the code exactly. A test that cannot fail proves nothing, and
   Covenant will check this itself afterwards: it swaps single operators in
   the protected code and rejects a check that notices none of them.
4. Write the shell command that runs just this test, as it would be typed from
   the current directory, to the file `{out_file}`. One line, nothing else.

Boundaries

- Do not change the code the promise protects to make the test pass. If the
  current code violates the promise, keep the honest failing test, still write
  the command, and explain what you found. Covenant reporting the promise as
  broken is the correct outcome in that case.
- If the code cannot be tested without a small change that leaves behaviour
  alone, such as exporting a function, make the smallest such change and say so
  in your summary. Covenant will ask a human to approve it.
- Do not edit covenant.toml and do not commit. Covenant updates covenant.toml
  itself after it has run your command and seen it pass.
- If the statement is too ambiguous to test, or this repository has nothing the
  statement applies to, do not write `{out_file}`. Explain what is missing.

Finish with a short summary for the human reviewer: the files you added or
changed, what the test asserts and over which inputs, how you confirmed it can
fail, and anything they should double-check.
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn promise(check: &str) -> Promise {
        Promise {
            id: "BILL-1".into(),
            statement: "An org is never billed more than its limit".into(),
            why: Some("Over-limit charges lose customers".into()),
            paths: vec!["src/billing/**".into(), "src/ledger/**".into()],
            check: check.into(),
            pack: None,
            timeout: None,
        }
    }

    #[test]
    fn prompt_carries_the_promise_and_the_output_file() {
        let p = prompt(&promise("false"), ".covenant-prove-BILL-1.check");
        assert!(p.contains("Promise BILL-1"));
        assert!(p.contains("Statement: An org is never billed more than its limit"));
        assert!(p.contains("Why it matters: Over-limit charges lose customers"));
        assert!(p.contains("src/billing/**, src/ledger/**"));
        assert!(p.contains("`.covenant-prove-BILL-1.check`"));
    }

    #[test]
    fn prompt_splits_a_pack_template_into_command_and_hint() {
        let p = prompt(
            &promise("cargo test --test bill_1 # TODO: drive random charges"),
            "out",
        );
        assert!(p.contains("placeholder to replace: `cargo test --test bill_1`"));
        assert!(p.contains("what the check must prove: drive random charges"));
    }

    #[test]
    fn prompt_omits_why_and_hint_when_absent() {
        let mut pr = promise("false");
        pr.why = None;
        let p = prompt(&pr, "out");
        assert!(!p.contains("Why it matters"));
        assert!(!p.contains("Hint from"));
    }
}
