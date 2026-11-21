use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::config::Promise;
use crate::error::Error;

pub const TRIGGER_ALL: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "package-lock.json",
    "foundry.toml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    All,
    Trigger(String),
    Paths,
    NoChanges,
}

#[derive(Debug)]
pub struct Selection<'a> {
    pub promises: Vec<&'a Promise>,
    pub reason: Reason,
}

pub fn select_all(promises: &[Promise]) -> Selection<'_> {
    Selection {
        promises: promises.iter().collect(),
        reason: Reason::All,
    }
}

pub fn select<'a>(promises: &'a [Promise], changed: &[String]) -> Result<Selection<'a>, Error> {
    if changed.is_empty() {
        return Ok(Selection {
            promises: Vec::new(),
            reason: Reason::NoChanges,
        });
    }
    if let Some(trigger) = changed.iter().find(|f| is_trigger(f)) {
        return Ok(Selection {
            promises: promises.iter().collect(),
            reason: Reason::Trigger(trigger.clone()),
        });
    }
    let mut selected = Vec::new();
    for p in promises {
        let set = glob_set(p)?;
        if changed.iter().any(|f| set.is_match(f)) {
            selected.push(p);
        }
    }
    Ok(Selection {
        promises: selected,
        reason: Reason::Paths,
    })
}

fn is_trigger(file: &str) -> bool {
    Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| TRIGGER_ALL.contains(&n))
}

pub fn glob_set(p: &Promise) -> Result<GlobSet, Error> {
    let mut b = GlobSetBuilder::new();
    for g in &p.paths {
        let glob = GlobBuilder::new(g)
            .literal_separator(true)
            .build()
            .map_err(|e| Error::BadGlob {
                id: p.id.clone(),
                glob: g.clone(),
                message: e.kind().to_string(),
            })?;
        b.add(glob);
    }
    b.build().map_err(|e| Error::BadGlob {
        id: p.id.clone(),
        glob: p.paths.join(", "),
        message: e.kind().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn promise(id: &str, paths: &[&str]) -> Promise {
        Promise {
            id: id.into(),
            statement: id.into(),
            why: None,
            paths: paths.iter().map(|s| s.to_string()).collect(),
            check: "true".into(),
            pack: None,
            timeout: None,
        }
    }

    fn files(f: &[&str]) -> Vec<String> {
        f.iter().map(|s| s.to_string()).collect()
    }

    fn ids<'a>(s: &'a Selection<'a>) -> Vec<&'a str> {
        s.promises.iter().map(|p| p.id.as_str()).collect()
    }

    fn fixture() -> Vec<Promise> {
        vec![
            promise("BILL", &["src/billing/**"]),
            promise("AUTH", &["src/auth/**", "migrations/*.sql"]),
            promise("API", &["src/api/**/*.rs"]),
        ]
    }

    #[test]
    fn no_changes_selects_nothing() {
        let p = fixture();
        let s = select(&p, &[]).unwrap();
        assert!(s.promises.is_empty());
        assert_eq!(s.reason, Reason::NoChanges);
    }

    #[test]
    fn selects_only_matching_promises() {
        let p = fixture();
        let s = select(&p, &files(&["src/billing/charge.rs"])).unwrap();
        assert_eq!(ids(&s), vec!["BILL"]);
        assert_eq!(s.reason, Reason::Paths);
    }

    #[test]
    fn double_star_matches_nested_directories() {
        let p = fixture();
        let s = select(&p, &files(&["src/billing/a/b/c.rs"])).unwrap();
        assert_eq!(ids(&s), vec!["BILL"]);
    }

    #[test]
    fn single_star_does_not_cross_directories() {
        let p = fixture();
        let s = select(&p, &files(&["migrations/2024/x.sql"])).unwrap();
        assert!(s.promises.is_empty());
        let s = select(&p, &files(&["migrations/x.sql"])).unwrap();
        assert_eq!(ids(&s), vec!["AUTH"]);
    }

    #[test]
    fn one_file_can_select_several_promises() {
        let p = vec![
            promise("A", &["src/**"]),
            promise("B", &["src/x/**"]),
            promise("C", &["other/**"]),
        ];
        let s = select(&p, &files(&["src/x/y.rs"])).unwrap();
        assert_eq!(ids(&s), vec!["A", "B"]);
    }

    #[test]
    fn unrelated_files_select_nothing() {
        let p = fixture();
        let s = select(&p, &files(&["README.md", "docs/x.md"])).unwrap();
        assert!(s.promises.is_empty());
        assert_eq!(s.reason, Reason::Paths);
    }

    #[test]
    fn lockfile_change_selects_everything() {
        let p = fixture();
        for trigger in [
            "Cargo.lock",
            "Cargo.toml",
            "package-lock.json",
            "foundry.toml",
            "crates/x/Cargo.toml",
        ] {
            let s = select(&p, &files(&["README.md", trigger])).unwrap();
            assert_eq!(ids(&s), vec!["BILL", "AUTH", "API"], "{trigger}");
            assert_eq!(s.reason, Reason::Trigger(trigger.to_string()));
        }
    }

    #[test]
    fn trigger_match_is_on_file_name_only() {
        let p = fixture();
        let s = select(&p, &files(&["src/Cargo.toml.bak", "Cargo.tomlx"])).unwrap();
        assert!(s.promises.is_empty());
    }

    #[test]
    fn select_all_ignores_changes() {
        let p = fixture();
        let s = select_all(&p);
        assert_eq!(ids(&s), vec!["BILL", "AUTH", "API"]);
        assert_eq!(s.reason, Reason::All);
    }

    #[test]
    fn invalid_glob_is_reported_with_promise_id() {
        let p = vec![promise("BAD", &["src/["])];
        let err = select(&p, &files(&["src/x"])).unwrap_err();
        assert!(matches!(err, Error::BadGlob { id, .. } if id == "BAD"));
    }
}
