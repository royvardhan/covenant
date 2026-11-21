# Covenant

An open registry of invariants ("promises") plus a small engine that runs the
right checks on every push and blocks the push if any promise breaks.

A promise is a sentence about your system that must stay true, paired with a
command that proves it. Covenant watches which files a change touches, runs
only the promises those files could break, and refuses the push on failure.
It is language-agnostic: a check is any command that exits 0 (kept) or
non-zero (broken).

```
$ git push
covenant: 1 changed file(s) since origin/main; 1 of 3 promise(s) affected

BLOCK  BILL-1  An org is never billed more than its limit in a billing period [billing]  (0.6s)
       $ cargo test -q -p billing-service --test bill_1
       seed 1: usage 8234 > limit 4775
       ...

0 PASS, 1 BLOCK  report: covenant-report.json
push blocked: a promise is broken
```

## The problem

Every codebase has a handful of properties that must never break: an org is
never billed over its limit, an order is never filled twice, a fee never
exceeds its quote. They live in people's heads, in a wiki nobody reads, or in a
test suite so large it is only run on CI, long after the push.

Covenant makes them explicit and cheap to enforce:

- **Named.** Each promise has an id and a plain-English statement, so a BLOCK
  tells you *which contract* you broke, not which test file.
- **Targeted.** Promises declare the paths that can break them. A docs change
  runs nothing; a change to `src/billing/` runs the billing promises; a
  lockfile change runs everything.
- **Shared.** Promises come in packs. The billing pack is the same for every
  billing system; you import it and wire each check to your code.

## Quickstart (2 minutes)

```sh
cargo install --path crates/covenant-cli   # from a clone of this repo

cd your-repo
covenant add billing          # copies 5 billing promises into covenant.toml
$EDITOR covenant.toml         # point each `check` at a real command
covenant check --all          # run every promise once
covenant hook install         # every `git push` now runs the affected promises
```

Or skip the editing and have an agent write each check:
`covenant prove BILL-1 --agent '<your coding agent>'` (see below).

That is it. From now on `git push` runs the promises whose `paths` overlap the
pushed diff and blocks on any failure. Run `covenant check --all` on main and
nightly; `.github/workflows/covenant.yml` in this repo does both.

Try it on the example instead:

```sh
cargo build
cd examples/billing-service
../../target/debug/covenant check --all                         # 3 PASS
git checkout demo/over-limit-bug
../../target/debug/covenant check --base main                   # BILL-1 BLOCK
git checkout main
```

## Promise format

`covenant.toml` lives at the root of the code it protects. Paths are relative
to it, and checks run from its directory.

```toml
timeout = 600                       # optional, seconds per check (default 600)
agent = "claude -p --allowedTools Read Write Edit Glob Grep Bash"   # optional, used by `covenant prove`

[[promise]]
id = "BILL-1"
statement = "An org is never billed more than its limit in a billing period"
why = "Over-limit charges are the billing bug customers notice first"   # optional
paths = ["src/billing/**"]          # globs; a change here selects this promise
check = "cargo test --test bill_1"  # any command; exit code is the verdict
pack = "billing"                    # optional, where it came from
timeout = 120                       # optional, overrides the top-level timeout
```

Selection rules:

1. Changed files come from `git diff --name-only <base>...<head>`.
   `--base` defaults to `@{upstream}`, then `origin/HEAD`, then `main`.
2. A promise runs if any changed file matches any of its `paths`.
   `*` does not cross `/`; `**` does.
3. If `Cargo.lock`, `Cargo.toml`, `package-lock.json` or `foundry.toml`
   changed anywhere, every promise runs.
4. `covenant check --all` runs every promise regardless of the diff.

Selected checks run in parallel, each with a timeout. A check that exits
non-zero is a BLOCK; one that exceeds its timeout is a TIMEOUT. Either makes
`covenant check` exit 1. Results are written to `covenant-report.json`.
On Unix a timed-out check is killed together with every process it started,
and so are running checks when `covenant check` is interrupted (exit 130).
Each check sees `COVENANT_PROMISE_ID` in its environment.

```
covenant check [--base <ref>] [--head <ref>] [--all] [--timeout <secs>] [--report <path>] [--quiet]
covenant list
covenant add <pack-name | path/to/pack.toml>
covenant prove <id> [--agent <command>] [--print-prompt] [--allow-source-edits] [--timeout <secs>]
covenant packs
covenant hook install [--force] | uninstall
```

Checks run against your working tree, so push the branch you have checked out.

## Letting an agent write the check

`covenant prove <id>` turns a promise's statement into a real check. It hands
the promise to a coding agent you already use, then verifies what comes back:

```sh
covenant add billing                 # BILL-1..5 arrive with placeholder checks
covenant prove BILL-1 --agent 'claude -p --allowedTools Read Write Edit Glob Grep Bash'
```

1. Covenant writes a brief (the statement, the why, the paths, the pack's hint)
   and pipes it to the agent command's stdin. `--print-prompt` shows it.
2. The agent reads the repo, writes a test in the repo's own framework, and
   reports the command that runs it.
3. Covenant lists every file the agent touched, runs the command, and only if
   it passes writes it into `covenant.toml` as the promise's `check`.

Covenant refuses the result if the agent edited existing files under the
promise's `paths`, because a check must not pass by changing the code it
guards; `--allow-source-edits` accepts such edits after you have read them.
Nothing is committed. You review the test once and commit it yourself.

The agent is any command that takes a prompt on stdin and can edit files and
run tests in the working directory. Set it with `--agent`, `COVENANT_AGENT`, or
`agent = "..."` at the top of `covenant.toml`. It also sees
`COVENANT_PROMISE_ID` and `COVENANT_PROVE_OUT` in its environment.

A check that passes once has not been shown to fail when the promise breaks.
The brief asks the agent to try that, but Covenant does not verify it yet
(see the roadmap).

## Pack format

A pack is a directory with a `pack.toml`. It is the same promise table with a
`why` for each promise and a check *template* the importer adapts. Packs never
set `pack =`; `covenant add` does that.

```toml
name = "billing"
description = "Invariants every metered or subscription billing system should keep"

[[promise]]
id = "BILL-3"
statement = "Retrying a charge with the same idempotency key charges the org exactly once"
why = "Clients retry on timeouts. Without idempotency a flaky network double-bills."
paths = ["src/billing/**", "src/api/charge*"]
check = "cargo test --test bill_3 # TODO: a test that submits the same key twice"
```

Built-in packs ship inside the binary and are listed by `covenant packs`:

| pack      | promises | covers |
|-----------|----------|--------|
| `billing` | BILL-1..5 | limits, non-negative usage, idempotent charges, invoice sums, no post-cancellation charges |
| `solver`  | SOLV-1..5 | min-output respected, at-most-once fills, deadlines, fee within quote, simulate before submit |

## Contributing a pack

1. Create `packs/<name>/pack.toml` with `name = "<name>"`, a description, and
   at least five promises. Each needs an `id` unique across all packs, a
   `statement` a non-engineer could agree with, a `why` that says what goes
   wrong when it breaks, `paths` that are plausible defaults for most repos,
   and a `check` template ending in a `# TODO` comment that says what the
   real check must prove.
2. Register it in `crates/covenant-cli/src/add.rs` so `covenant add <name>` works.
3. Run `cargo test`. The `packs` test validates every pack, and the repo's own
   `covenant.toml` runs it on any change under `packs/`.
4. Open a pull request. Promises should be invariants (always true), not
   policies (true until the product changes). "Usage is never negative" is a
   promise; "the free tier is 1000 requests" is a config value.

## Repo layout

```
crates/covenant-engine/   load promises, select by diff, run checks, write the report
crates/covenant-cli/      covenant check | list | add | packs | prove | hook
packs/billing/            generic billing promises
packs/solver/             intent / order-flow solver promises
examples/billing-service/ tiny ledger with 3 wired promises; demo/over-limit-bug shows a BLOCK
covenant.toml             this repo's own promises, run by the hook and the Action
```

## Roadmap

The goal is a verification layer: you state what must stay true, and Covenant
produces and enforces the proof. Today a human writes every check.

- v0.2: statement to check. `covenant prove <id>` has an AI agent read the repo
  and write the test that proves a promise; a human reviews it once. The first
  version is in: one promise at a time, through an external agent command
- v0.3: proof the check is real. Break the code on purpose (seeded fault
  injection) and require the check to fail, so a generated check cannot pass
  vacuously
- v0.4: diff to promise. On each change the agent flags the promises it
  endangers and proposes new ones, replacing hand-written `paths` globs
  (dependency-graph selection lands here as the non-AI fallback)
- v0.5: packs with real content. A pack ships the guidance needed to generate
  working checks for a given stack, not `# TODO` templates

## License

MIT or Apache-2.0, at your option.
