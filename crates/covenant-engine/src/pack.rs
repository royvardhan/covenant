use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{Config, Promise};
use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    pub description: String,
    #[serde(rename = "promise")]
    pub promises: Vec<Promise>,
}

impl Pack {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text).map_err(|e| match e {
            Error::Parse { message, .. } => Error::Parse {
                path: path.to_path_buf(),
                message,
            },
            other => other,
        })
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let pack: Pack = toml::from_str(text).map_err(|e| Error::Parse {
            path: "pack.toml".into(),
            message: e.message().to_string(),
        })?;
        Config {
            promises: pack.promises.clone(),
            ..Config::default()
        }
        .validate()?;
        Ok(pack)
    }
}

impl Config {
    pub fn add_pack(&mut self, pack: &Pack) -> Vec<String> {
        let mut added = Vec::new();
        for p in &pack.promises {
            if self.get(&p.id).is_some() {
                continue;
            }
            let mut p = p.clone();
            p.pack = Some(pack.name.clone());
            added.push(p.id.clone());
            self.promises.push(p);
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Pack {
        Pack::parse(
            r#"
            name = "demo"
            description = "d"
            [[promise]]
            id = "D-1"
            statement = "a"
            paths = ["a/**"]
            check = "true"
            [[promise]]
            id = "D-2"
            statement = "b"
            paths = ["b/**"]
            check = "true"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn add_pack_tags_promises_with_pack_name() {
        let mut c = Config::default();
        let added = c.add_pack(&pack());
        assert_eq!(added, vec!["D-1", "D-2"]);
        assert!(c.promises.iter().all(|p| p.pack.as_deref() == Some("demo")));
    }

    #[test]
    fn add_pack_skips_ids_already_present() {
        let mut c = Config::default();
        c.add_pack(&pack());
        let added = c.add_pack(&pack());
        assert!(added.is_empty());
        assert_eq!(c.promises.len(), 2);
    }
}
