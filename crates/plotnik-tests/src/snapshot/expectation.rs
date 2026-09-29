//! Authored requirements, kept separate from generated observations.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub admission: Admission,
    pub tree: Option<String>,
    pub runtime: Option<Runtime>,
    pub codes: Option<Vec<String>>,
    pub diagnostics: Option<Value>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Admission {
    Accept,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "result",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Runtime {
    Match(Value),
    NoMatch,
}

impl Expectation {
    pub fn load(snapshot: &Path) -> Result<Option<Self>, String> {
        let path = snapshot.with_extension("expect.json");
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read {}: {error}", path.display())),
        };
        let expected = serde_json::from_str(&raw)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        Ok(Some(expected))
    }

    pub fn check_diagnostics(&self, actual: &Value) -> Result<(), String> {
        let diagnostics = actual.as_array().ok_or("diagnostics must be an array")?;
        let admission = if diagnostics.iter().any(|d| d["severity"] == "error") {
            Admission::Reject
        } else {
            Admission::Accept
        };
        if admission != self.admission {
            return Err(format!(
                "expected {:?}, observed {admission:?}",
                self.admission
            ));
        }

        if let Some(expected) = &self.codes {
            let codes = diagnostics
                .iter()
                .map(|d| d["code"].as_str().ok_or("diagnostic code must be a string"))
                .collect::<Result<Vec<_>, _>>()?;
            if codes != *expected {
                return Err(format!(
                    "expected diagnostic codes {expected:?}, observed {codes:?}"
                ));
            }
        }

        if let Some(expected) = &self.diagnostics
            && actual != expected
        {
            return Err(format!(
                "diagnostic payload violates the authored expectation:\nexpected {expected}\nobserved {actual}"
            ));
        }

        Ok(())
    }
}

impl Runtime {
    pub fn check(&self, actual: &str) -> Result<(), String> {
        match self {
            Self::NoMatch if actual == "<no match>" => Ok(()),
            Self::NoMatch => Err(format!("expected no match, observed {actual}")),
            Self::Match(expected) => {
                let value: Value = serde_json::from_str(actual)
                    .map_err(|_| format!("expected match {expected}, observed {actual}"))?;
                if value != *expected {
                    return Err(format!("expected match {expected}, observed {value}"));
                }

                Ok(())
            }
        }
    }
}
