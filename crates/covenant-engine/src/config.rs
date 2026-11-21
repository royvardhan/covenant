use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Promise {
    pub id: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub paths: Vec<String>,
    pub check: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Command `covenant prove` hands a promise to; it reads the brief on stdin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, rename = "promise")]
    pub promises: Vec<Promise>,
}

impl Config {
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
        let config: Config = toml::from_str(text).map_err(|e| Error::Parse {
            path: "covenant.toml".into(),
            message: e.message().to_string(),
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.promises {
            if !seen.insert(p.id.as_str()) {
                return Err(Error::DuplicateId(p.id.clone()));
            }
            if p.paths.is_empty() {
                return Err(Error::EmptyPaths(p.id.clone()));
            }
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("config serializes")
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        fs::write(path, self.to_toml()).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn get(&self, id: &str) -> Option<&Promise> {
        self.promises.iter().find(|p| p.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_promise() {
        let c = Config::parse(
            r#"
            [[promise]]
            id = "X-1"
            statement = "x holds"
            paths = ["src/**"]
            check = "true"
            "#,
        )
        .unwrap();
        assert_eq!(c.promises.len(), 1);
        assert_eq!(c.promises[0].id, "X-1");
        assert_eq!(c.promises[0].pack, None);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let err = Config::parse(
            r#"
            [[promise]]
            id = "X-1"
            statement = "a"
            paths = ["a"]
            check = "true"
            [[promise]]
            id = "X-1"
            statement = "b"
            paths = ["b"]
            check = "true"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, Error::DuplicateId(id) if id == "X-1"));
    }

    #[test]
    fn rejects_empty_paths() {
        let err = Config::parse(
            r#"
            [[promise]]
            id = "X-1"
            statement = "a"
            paths = []
            check = "true"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, Error::EmptyPaths(_)));
    }

    #[test]
    fn round_trips_through_toml() {
        let c = Config {
            timeout: Some(30),
            agent: Some("my-agent --run".into()),
            promises: vec![Promise {
                id: "A-1".into(),
                statement: "s".into(),
                why: Some("w".into()),
                paths: vec!["src/**".into()],
                check: "true".into(),
                pack: Some("p".into()),
                timeout: None,
            }],
        };
        assert_eq!(Config::parse(&c.to_toml()).unwrap(), c);
    }
}
