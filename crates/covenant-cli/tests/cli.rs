use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn covenant() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_covenant"))
}

struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("covenant-cli-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let repo = Repo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "t@example.com"]);
        repo.git(&["config", "user.name", "t"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, path: &str, content: &str) {
        let p = self.dir.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn commit(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", msg]);
    }

    fn covenant(&self, args: &[&str]) -> Output {
        Command::new(covenant())
            .current_dir(&self.dir)
            .env("NO_COLOR", "1")
            .args(args)
            .output()
            .unwrap()
    }

    fn report(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.dir.join("covenant-report.json")).unwrap())
            .unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const CONFIG: &str = r#"
[[promise]]
id = "OK-1"
statement = "billing passes"
paths = ["src/billing/**"]
check = "test -f src/billing/ok"

[[promise]]
id = "BAD-1"
statement = "auth passes"
paths = ["src/auth/**"]
check = "echo auth is broken; exit 7"
"#;

fn seed(repo: &Repo) {
    repo.write("covenant.toml", CONFIG);
    repo.write("src/billing/ok", "");
    repo.write("src/auth/mod.rs", "");
    repo.write("README.md", "");
    repo.commit("init");
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn passing_promise_exits_zero_and_writes_report() {
    let repo = Repo::new("pass");
    seed(&repo);
    repo.write("src/billing/charge.rs", "x");
    repo.commit("touch billing");

    let out = repo.covenant(&["check", "--base", "HEAD~1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("PASS   OK-1"), "{text}");
    assert!(!text.contains("BAD-1"), "{text}");

    let report = repo.report();
    assert_eq!(report["reason"].as_str().unwrap(), "paths");
    assert_eq!(report["changed_files"].as_array().unwrap().len(), 1);
    assert_eq!(report["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["results"].as_array().unwrap()[0]["outcome"]
            .as_str()
            .unwrap(),
        "PASS"
    );
}

#[test]
fn failing_promise_blocks_with_exit_one_and_shows_output() {
    let repo = Repo::new("block");
    seed(&repo);
    repo.write("src/auth/login.rs", "x");
    repo.commit("touch auth");

    let out = repo.covenant(&["check", "--base", "HEAD~1"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("BLOCK  BAD-1"), "{text}");
    assert!(text.contains("auth is broken"), "{text}");
    assert!(text.contains("push blocked"), "{text}");

    let r = repo.report();
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["outcome"].as_str().unwrap(), "BLOCK");
    assert_eq!(results[0]["exit_code"].as_i64().unwrap(), 7);
}

#[test]
fn unrelated_change_runs_nothing() {
    let repo = Repo::new("unrelated");
    seed(&repo);
    repo.write("README.md", "changed");
    repo.commit("docs");

    let out = repo.covenant(&["check", "--base", "HEAD~1"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("0 of 2 promise(s) affected"),
        "{}",
        stdout(&out)
    );
    assert_eq!(repo.report()["results"].as_array().unwrap().len(), 0);
}

#[test]
fn lockfile_change_runs_everything() {
    let repo = Repo::new("lockfile");
    seed(&repo);
    repo.write("Cargo.lock", "x");
    repo.commit("deps");

    let out = repo.covenant(&["check", "--base", "HEAD~1"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("`Cargo.lock` changed"), "{text}");
    assert!(text.contains("PASS   OK-1"), "{text}");
    assert!(text.contains("BLOCK  BAD-1"), "{text}");
    assert_eq!(
        repo.report()["reason"].as_str().unwrap(),
        "trigger:Cargo.lock"
    );
}

#[test]
fn all_flag_ignores_git_state() {
    let repo = Repo::new("all");
    seed(&repo);
    let out = repo.covenant(&["check", "--all"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(repo.report()["results"].as_array().unwrap().len(), 2);
    assert_eq!(repo.report()["reason"].as_str().unwrap(), "all");
}

#[test]
fn timeout_is_reported_and_blocks() {
    let repo = Repo::new("timeout");
    repo.write(
        "covenant.toml",
        r#"
[[promise]]
id = "SLOW-1"
statement = "slow"
paths = ["**"]
check = "sleep 10"
"#,
    );
    repo.commit("init");
    let out = repo.covenant(&["check", "--all", "--timeout", "1"]);
    assert_ne!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("TIMEOUT"), "{}", stdout(&out));
    assert_eq!(
        repo.report()["results"].as_array().unwrap()[0]["outcome"]
            .as_str()
            .unwrap(),
        "TIMEOUT"
    );
}

#[cfg(unix)]
#[test]
fn interrupt_kills_running_checks_and_exits_130() {
    use std::time::{Duration, Instant};

    let repo = Repo::new("interrupt");
    repo.write(
        "covenant.toml",
        r#"
[[promise]]
id = "SLOW-1"
statement = "slow"
paths = ["**"]
check = "sleep 30 & echo $! > sleep.pid; wait"
"#,
    );
    repo.commit("init");

    let mut child = Command::new(covenant())
        .current_dir(&repo.dir)
        .env("NO_COLOR", "1")
        .args(["check", "--all"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let alive = |pid: &str| {
        Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    let wait_for = |what: &str, cond: &mut dyn FnMut() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(25));
        }
    };

    let pid_file = repo.dir.join("sleep.pid");
    let mut sleep_pid = String::new();
    wait_for("the check to start", &mut || {
        sleep_pid = fs::read_to_string(&pid_file)
            .unwrap_or_default()
            .trim()
            .to_string();
        !sleep_pid.is_empty()
    });
    assert!(alive(&sleep_pid));

    // Signal covenant only, as a non-terminal sender would: the check must not
    // rely on the terminal delivering the interrupt to it too.
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());

    let mut exit = None;
    wait_for("covenant to exit", &mut || {
        exit = child.try_wait().unwrap();
        exit.is_some()
    });
    assert_eq!(exit.unwrap().code(), Some(130));
    wait_for("the check's child to die", &mut || !alive(&sleep_pid));
}

#[test]
fn checks_run_from_the_config_directory() {
    let repo = Repo::new("subdir");
    repo.write(
        "svc/covenant.toml",
        r#"
[[promise]]
id = "SVC-1"
statement = "relative paths and cwd"
paths = ["src/**"]
check = "test -f src/lib.rs && test $COVENANT_PROMISE_ID = SVC-1"
"#,
    );
    repo.write("svc/src/lib.rs", "");
    repo.write("other/x", "");
    repo.commit("init");
    repo.write("svc/src/lib.rs", "changed");
    repo.write("other/x", "changed");
    repo.commit("change");

    let out = repo.covenant(&[
        "--config",
        "svc/covenant.toml",
        "check",
        "--base",
        "HEAD~1",
        "--report",
        "covenant-report.json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let r = repo.report();
    assert_eq!(r["changed_files"].as_array().unwrap().len(), 1);
    assert_eq!(
        r["changed_files"].as_array().unwrap()[0].as_str().unwrap(),
        "src/lib.rs"
    );
}

#[test]
fn missing_config_is_a_usage_error() {
    let repo = Repo::new("noconfig");
    let out = repo.covenant(&["check", "--all"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("covenant.toml not found"));
}

#[test]
fn hook_install_writes_executable_pre_push() {
    let repo = Repo::new("hook");
    seed(&repo);
    let out = repo.covenant(&["hook", "install"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let hook = repo.dir.join(".git/hooks/pre-push");
    let text = fs::read_to_string(&hook).unwrap();
    assert!(text.contains("covenant check --base"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(fs::metadata(&hook).unwrap().permissions().mode() & 0o111 != 0);
    }

    let out = repo.covenant(&["hook", "install"]);
    assert_eq!(out.status.code(), Some(0));

    fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    let out = repo.covenant(&["hook", "install"]);
    assert_eq!(out.status.code(), Some(2));
    let out = repo.covenant(&["hook", "install", "--force"]);
    assert_eq!(out.status.code(), Some(0));

    let out = repo.covenant(&["hook", "uninstall"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!Path::new(&hook).exists());
}

#[test]
fn add_copies_a_builtin_pack_and_is_idempotent() {
    let repo = Repo::new("add");
    let out = repo.covenant(&["add", "billing"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("added 5 promise(s) from `billing`"),
        "{}",
        stdout(&out)
    );

    let text = fs::read_to_string(repo.dir.join("covenant.toml")).unwrap();
    assert!(text.contains("id = \"BILL-1\""));
    assert!(text.contains("pack = \"billing\""));

    let out = repo.covenant(&["add", "billing"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("already in"));

    let out = repo.covenant(&["add", "solver"]);
    assert_eq!(out.status.code(), Some(0));
    let out = repo.covenant(&["list"]);
    let text = stdout(&out);
    assert!(text.contains("BILL-5") && text.contains("SOLV-5"), "{text}");

    let out = repo.covenant(&["add", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("built-in packs: billing, solver"));
}

#[test]
fn add_accepts_a_path_to_a_pack() {
    let repo = Repo::new("addpath");
    repo.write(
        "mypack/pack.toml",
        r#"
name = "mine"
description = "local"
[[promise]]
id = "MINE-1"
statement = "it holds"
paths = ["src/**"]
check = "true"
"#,
    );
    let out = repo.covenant(&["add", "mypack"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(repo.dir.join("covenant.toml")).unwrap();
    assert!(
        text.contains("MINE-1") && text.contains("pack = \"mine\""),
        "{text}"
    );
}
