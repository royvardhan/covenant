use std::fs;
use std::path::PathBuf;

use covenant_engine::Pack;

fn packs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn packs() -> Vec<(String, Pack)> {
    let mut out: Vec<_> = fs::read_dir(packs_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .map(|dir| {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            let pack = Pack::load(&dir.join("pack.toml")).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, pack)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_pack_parses_and_is_named_after_its_directory() {
    let packs = packs();
    assert!(packs.len() >= 2, "expected the billing and solver packs");
    for (dir, pack) in &packs {
        assert_eq!(&pack.name, dir);
        assert!(
            !pack.description.trim().is_empty(),
            "{dir}: empty description"
        );
    }
}

#[test]
fn every_promise_has_a_statement_a_why_and_a_check_template() {
    for (dir, pack) in packs() {
        assert!(pack.promises.len() >= 5, "{dir}: fewer than 5 promises");
        for p in &pack.promises {
            assert!(
                !p.statement.trim().is_empty(),
                "{dir}/{}: empty statement",
                p.id
            );
            assert!(
                p.why.as_deref().is_some_and(|w| !w.trim().is_empty()),
                "{dir}/{}: missing why",
                p.id
            );
            assert!(!p.paths.is_empty(), "{dir}/{}: no paths", p.id);
            assert!(!p.check.trim().is_empty(), "{dir}/{}: empty check", p.id);
            assert!(
                p.pack.is_none(),
                "{dir}/{}: `pack` is set by `covenant add`, not in the pack",
                p.id
            );
        }
    }
}

#[test]
fn promise_ids_are_unique_across_all_packs() {
    let mut seen = std::collections::HashSet::new();
    for (dir, pack) in packs() {
        for p in &pack.promises {
            assert!(
                seen.insert(p.id.clone()),
                "{dir}/{}: id reused by another pack",
                p.id
            );
        }
    }
}
