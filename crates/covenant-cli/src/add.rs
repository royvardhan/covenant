use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use covenant_engine::{Config, Pack};

const BUILTIN: &[(&str, &str)] = &[
    ("billing", include_str!("../../../packs/billing/pack.toml")),
    ("solver", include_str!("../../../packs/solver/pack.toml")),
];

pub fn run(config_path: &Path, name: &str) -> Result<ExitCode> {
    let pack = load_pack(name)?;
    let mut cfg = if config_path.exists() {
        Config::load(config_path)
            .with_context(|| format!("could not load {}", config_path.display()))?
    } else {
        Config::default()
    };
    let added = cfg.add_pack(&pack);
    if added.is_empty() {
        println!(
            "every promise from `{}` is already in {}",
            pack.name,
            config_path.display()
        );
        return Ok(ExitCode::SUCCESS);
    }
    cfg.save(config_path)?;
    println!(
        "added {} promise(s) from `{}` to {}:",
        added.len(),
        pack.name,
        config_path.display()
    );
    for id in &added {
        let p = cfg.get(id).expect("just added");
        println!("  {id}  {}", p.statement);
    }
    println!();
    println!(
        "next: point each promise's `paths` at your code in {}, then run `covenant prove <id>` \
         to have its check written, or write the `check` command yourself",
        config_path.display()
    );
    Ok(ExitCode::SUCCESS)
}

pub fn list() -> Result<ExitCode> {
    for (name, text) in BUILTIN {
        let pack = Pack::parse(text)?;
        println!(
            "{name}  ({} promises)  {}",
            pack.promises.len(),
            pack.description
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn load_pack(name: &str) -> Result<Pack> {
    if let Some((_, text)) = BUILTIN.iter().find(|(n, _)| *n == name) {
        return Pack::parse(text).with_context(|| format!("built-in pack `{name}` is invalid"));
    }
    let path = Path::new(name);
    let file = if path.is_dir() {
        path.join("pack.toml")
    } else {
        path.to_path_buf()
    };
    if file.exists() {
        return Pack::load(&file)
            .with_context(|| format!("could not load pack {}", file.display()));
    }
    let names: Vec<&str> = BUILTIN.iter().map(|(n, _)| *n).collect();
    bail!(
        "no pack named `{name}`; built-in packs: {}. Or pass a path to a pack.toml",
        names.join(", ")
    )
}
