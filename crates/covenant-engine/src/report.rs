use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Outcome {
    Pass,
    Block,
    Timeout,
}

impl Outcome {
    pub fn is_pass(self) -> bool {
        self == Outcome::Pass
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Outcome::Pass => "PASS",
            Outcome::Block => "BLOCK",
            Outcome::Timeout => "TIMEOUT",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromiseResult {
    pub id: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    pub check: String,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub generated_at_unix: u64,
    pub base: Option<String>,
    pub head: Option<String>,
    pub reason: String,
    pub changed_files: Vec<String>,
    pub total_promises: usize,
    pub results: Vec<PromiseResult>,
}

impl Report {
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn blocked(&self) -> bool {
        self.results.iter().any(|r| !r.outcome.is_pass())
    }

    pub fn count(&self, outcome: Outcome) -> usize {
        self.results.iter().filter(|r| r.outcome == outcome).count()
    }

    pub fn write(&self, path: &Path) -> Result<(), Error> {
        let json = serde_json::to_string_pretty(self).expect("report serializes");
        fs::write(path, json).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
