//! Small, deliberate breakages ("mutants") of the code a promise protects.
//!
//! A check that still passes on every mutant has not been shown to notice
//! anything. Mutants are single-token swaps that keep the code well-formed in
//! most languages (`>` for `>=`, `==` for `!=`, `&&` for `||`, `+` for `-`,
//! `true` for `false`), found by scanning text, so this needs no parser and
//! works on any language at the cost of some noise.

/// Whitespace-delimited operator tokens and what each is swapped for.
const OPERATORS: &[(&str, &str)] = &[
    (">=", ">"),
    (">", ">="),
    ("<=", "<"),
    ("<", "<="),
    ("==", "!="),
    ("!=", "=="),
    ("===", "!=="),
    ("!==", "==="),
    ("&&", "||"),
    ("||", "&&"),
    ("+", "-"),
    ("-", "+"),
    ("+=", "-="),
    ("-=", "+="),
];

/// Boolean literals, matched as whole words.
const WORDS: &[(&str, &str)] = &[
    ("true", "false"),
    ("false", "true"),
    ("True", "False"),
    ("False", "True"),
];

const COMMENT_STARTS: &[&str] = &["//", "#", "*", "/*", "--", ";", "<!--"];

/// One place in one file where a token can be swapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    /// 1-based.
    pub line: usize,
    /// Byte offset of the token in the file.
    pub offset: usize,
    pub from: &'static str,
    pub to: &'static str,
}

/// Paths that hold tests, not the code under test. Breaking a test makes it
/// fail without showing that it watches the code.
pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let file = parts.pop().unwrap_or_default();
    if parts.iter().any(|d| {
        matches!(
            *d,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "testdata" | "fixtures"
        )
    }) {
        return true;
    }
    let stem = file.split('.').next().unwrap_or_default();
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || file.contains(".test.")
        || file.contains(".spec.")
}

/// Manifests, data and prose. A swapped token there is a broken build or a
/// changed sentence, not a bug in the behaviour a promise describes.
pub fn is_code_path(path: &str) -> bool {
    const NOT_CODE: &[&str] = &[
        "toml", "json", "yaml", "yml", "lock", "md", "txt", "xml", "html", "css", "csv", "svg",
        "ini", "cfg", "conf", "env",
    ];
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            !NOT_CODE.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => true,
    }
}

/// Every mutation site in `text`, in file order.
pub fn sites(file: &str, text: &str) -> Vec<Site> {
    let rust = file.ends_with(".rs");
    let mut out = Vec::new();
    let mut line_start = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let start = line_start;
        line_start += line.len();
        let trimmed = line.trim_start();
        // Inline Rust unit tests sit below this marker, to the end of the file.
        if rust && trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.is_empty() || COMMENT_STARTS.iter().any(|c| trimmed.starts_with(c)) {
            continue;
        }
        let code_end = line.find("// ").unwrap_or(line.len());
        let code = &line[..code_end];

        let tokens: Vec<(usize, &str)> = tokens(code).collect();
        for (n, &(at, tok)) in tokens.iter().enumerate() {
            let Some(&(from, to)) = OPERATORS.iter().find(|(f, _)| *f == tok) else {
                continue;
            };
            // A leading `-`, `+`, `>` or `<` is a list bullet, a diff marker or
            // a quote far more often than an operator.
            if n == 0 && matches!(tok, "-" | "+" | ">" | "<") {
                continue;
            }
            // `move || {` and `= || {` are closures, not logical ors.
            if tok == "||" {
                let closure = match n.checked_sub(1).map(|p| tokens[p].1) {
                    None => true,
                    Some(prev) => ["move", "(", "=", ",", "return"]
                        .iter()
                        .any(|s| prev.ends_with(s)),
                };
                if closure {
                    continue;
                }
            }
            if in_string(code, at) {
                continue;
            }
            out.push(Site {
                file: file.to_string(),
                line: i + 1,
                offset: start + at,
                from,
                to,
            });
        }

        for &(from, to) in WORDS {
            for (at, _) in code.match_indices(from) {
                let before = code[..at].chars().next_back();
                let after = code[at + from.len()..].chars().next();
                let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
                if word(before) || word(after) || in_string(code, at) {
                    continue;
                }
                out.push(Site {
                    file: file.to_string(),
                    line: i + 1,
                    offset: start + at,
                    from,
                    to,
                });
            }
        }
    }
    out.sort_by_key(|s| s.offset);
    out
}

fn tokens(line: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = line;
    let mut base = 0;
    std::iter::from_fn(move || {
        let skipped = rest.len() - rest.trim_start().len();
        base += skipped;
        rest = &rest[skipped..];
        if rest.is_empty() {
            return None;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let tok = (base, &rest[..len]);
        base += len;
        rest = &rest[len..];
        Some(tok)
    })
}

/// True when an odd number of double quotes precede `at`: inside a string.
fn in_string(line: &str, at: usize) -> bool {
    line[..at].matches('"').count() % 2 == 1
}

/// `text` with the token at `site` swapped. `None` if the file no longer has
/// that token there.
pub fn apply(text: &str, site: &Site) -> Option<String> {
    let end = site.offset + site.from.len();
    if text.get(site.offset..end) != Some(site.from) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 1);
    out.push_str(&text[..site.offset]);
    out.push_str(site.to);
    out.push_str(&text[end..]);
    Some(out)
}

/// Deterministically shuffle `sites` by `seed`.
pub fn shuffle(sites: &mut [Site], seed: u64) {
    let mut state = seed;
    let mut next = move || {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for i in (1..sites.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        sites.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swaps(file: &str, text: &str) -> Vec<(usize, &'static str, &'static str)> {
        sites(file, text)
            .into_iter()
            .map(|s| (s.line, s.from, s.to))
            .collect()
    }

    #[test]
    fn finds_operators_and_booleans() {
        let text = "if used + amount > limit {\n    return false;\n}\n";
        assert_eq!(
            swaps("a.rs", text),
            vec![(1, "+", "-"), (1, ">", ">="), (2, "false", "true")]
        );
    }

    #[test]
    fn leaves_arrows_generics_comments_and_strings_alone() {
        let text = concat!(
            "fn f(x: Vec<u8>) -> bool {\n",
            "    // a > b is a comment\n",
            "    let s = \"a > b\";\n",
            "    let t = x.len(); // trailing > comment\n",
            "    match t { 0 => truthy, _ => other }\n",
            "    thread::spawn(move || { run() });\n",
            "}\n",
        );
        assert_eq!(swaps("a.rs", text), vec![]);
    }

    #[test]
    fn skips_leading_bullets_but_keeps_leading_logical_operators() {
        let text = "- item one\n> quoted\n    && ready\n    a || done\n";
        assert_eq!(
            swaps("notes.txt", text),
            vec![(3, "&&", "||"), (4, "||", "&&")]
        );
    }

    #[test]
    fn stops_at_inline_rust_tests_only_in_rust_files() {
        let text = "let a = b == c;\n#[cfg(test)]\nmod tests { fn t() { assert!(a == b); } }\n";
        assert_eq!(swaps("a.rs", text).len(), 1);
        assert_eq!(swaps("a.py", text).len(), 2);
    }

    #[test]
    fn apply_swaps_exactly_one_token() {
        let text = "a > b && c > d\n";
        let all = sites("f", text);
        assert_eq!(all.len(), 3);
        assert_eq!(apply(text, &all[2]).unwrap(), "a > b && c >= d\n");
        assert_eq!(apply(text, &all[1]).unwrap(), "a > b || c > d\n");
        assert_eq!(apply("changed\n", &all[2]), None);
    }

    #[test]
    fn shuffle_is_deterministic_per_seed() {
        let text = "a > b\nc < d\ne == f\ng != h\ni >= j\nk <= l\n";
        let base = sites("f", text);
        let (mut one, mut two, mut other) = (base.clone(), base.clone(), base.clone());
        shuffle(&mut one, 7);
        shuffle(&mut two, 7);
        shuffle(&mut other, 8);
        assert_eq!(one, two);
        assert_ne!(one, other);
        assert_ne!(one, base);
    }

    #[test]
    fn manifests_and_prose_are_not_code() {
        for p in ["Cargo.toml", "pkg/package.json", "README.md", "a/b.YAML"] {
            assert!(!is_code_path(p), "{p}");
        }
        for p in ["src/a.rs", "bin/run", "scripts/charge.sh", ".hidden"] {
            assert!(is_code_path(p), "{p}");
        }
    }

    #[test]
    fn recognises_test_paths() {
        for p in [
            "tests/bill_1.rs",
            "src/__tests__/a.ts",
            "src/ledger.test.ts",
            "pkg/ledger_test.go",
            "test_ledger.py",
            "spec/ledger_spec.rb",
        ] {
            assert!(is_test_path(p), "{p}");
        }
        for p in [
            "src/ledger.rs",
            "src/contest.rs",
            "src/inspect.ts",
            "latest/a.py",
        ] {
            assert!(!is_test_path(p), "{p}");
        }
    }
}
