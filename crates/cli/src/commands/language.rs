use std::fs;
use std::path::{Path, PathBuf};

use clap::ArgMatches;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use plotnik_compiler::WasmParser;

use super::grammar;
use crate::error::CliError;

const ENDPOINT: &str = "https://plotnik.dev/lang";
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;
const PAIR_ATTEMPTS: usize = 3;
const MAX_ERROR_BODY: u64 = 8 * 1024;
const CACHE_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'@');

#[derive(Debug, PartialEq, Eq)]
struct Selection {
    name: String,
    revision: Option<String>,
}

impl Selection {
    fn parse(input: &str) -> Result<Self, CliError> {
        let (name, revision) = match input.split_once('@') {
            Some((name, revision)) => {
                if revision.is_empty()
                    || revision.contains('@')
                    || revision.chars().any(char::is_whitespace)
                    || revision.chars().any(char::is_control)
                {
                    return Err(CliError::fatal(
                        "expected language@revision with a nonempty revision",
                    ));
                }

                let revision =
                    if revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()) {
                        revision.to_ascii_lowercase()
                    } else {
                        revision.to_owned()
                    };
                (name, Some(revision))
            }
            None => (input, None),
        };
        if name.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains('\\')
                || part.chars().any(char::is_control)
        }) {
            return Err(CliError::fatal(
                "expected language alias or owner/repo[/grammar-directory] without empty or traversal components",
            ));
        }

        let name = match name {
            "js" | "jsx" | "mjs" | "cjs" => "javascript",
            "ts" => "typescript",
            "py" => "python",
            "rs" => "rust",
            "h" => "c",
            other => other,
        };
        Ok(Self {
            name: name.to_owned(),
            revision,
        })
    }
}

struct Artifact {
    build: String,
    commit: String,
    bytes: Vec<u8>,
}

pub struct Artifacts {
    pub grammar: plotnik_compiler::grammar::Grammar,
    parser: WasmParser,
}

impl Artifacts {
    pub fn parser(self) -> WasmParser {
        self.parser
    }
}

pub fn load(m: &ArgMatches, source: Option<&Path>) -> Result<Artifacts, CliError> {
    let input = m
        .get_one::<String>("lang")
        .map(String::as_str)
        .or_else(|| source?.extension()?.to_str())
        .ok_or_else(|| CliError::fatal("source language required: use --lang"))?;
    let selection = Selection::parse(input)?;
    let cache = cache_path(&cache_dir(m)?, &selection);
    if let Some(artifacts) = read_cache(&cache)? {
        return Ok(artifacts);
    }

    let path = selection
        .name
        .split('/')
        .map(|part| utf8_percent_encode(part, NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("{ENDPOINT}/{path}");
    let query = match &selection.revision {
        Some(revision) => format!("?ref={}", utf8_percent_encode(revision, NON_ALPHANUMERIC)),
        None => String::new(),
    };
    let json_url = format!("{url}/grammar.json{query}");

    // Each GET can resolve a moving branch or a new toolchain build. Never mix them.
    for _ in 0..PAIR_ATTEMPTS {
        let json = fetch(&json_url)?;
        let wasm = fetch(&format!("{url}/parser.wasm{query}"))?;
        if json.build != wasm.build || json.commit != wasm.commit {
            continue;
        }
        if let Some(revision) = &selection.revision
            && is_hex(revision, 40)
            && json.commit != *revision
        {
            return Err(CliError::fatal(
                "service returned a different commit than requested",
            ));
        }

        return write_cache(&cache, json.bytes, wasm.bytes);
    }
    Err(CliError::fatal(format!(
        "service artifact build/commit mismatch after {PAIR_ATTEMPTS} attempts",
    )))
}

fn fetch(url: &str) -> Result<Artifact, CliError> {
    let mut response = request(url)?;
    if response.status() != 200 {
        let status = response.status();
        let retry = match response.headers().get("Retry-After") {
            Some(value) => {
                let value = value.to_str().map_err(|error| {
                    CliError::fatal(format!(
                        "failed to fetch {url}: HTTP {status}; invalid Retry-After: {error}"
                    ))
                })?;
                format!("; Retry-After: {value}")
            }
            None => String::new(),
        };
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_ERROR_BODY)
            .read_to_vec()
            .map_err(|error| {
                CliError::fatal(format!(
                    "failed to fetch {url}: HTTP {status}{retry}; {error}"
                ))
            })?;
        let detail = serde_json::from_slice::<serde_json::Value>(&body);
        let message = if let Ok(detail) = &detail
            && let Some(code) = detail["error"]["code"].as_str()
            && let Some(message) = detail["error"]["message"].as_str()
        {
            let stage = match detail["error"]["stage"].as_str() {
                Some(stage) => format!(" ({stage})"),
                None => String::new(),
            };
            format!(": {code}{stage}: {message}")
        } else {
            String::new()
        };
        return Err(CliError::fatal(format!(
            "failed to fetch {url}: HTTP {status}{message}{retry}"
        )));
    }
    let headers = response.headers();
    let header = |name, len| -> Result<String, CliError> {
        let values = headers.get_all(name).iter().collect::<Vec<_>>();
        let [value] = values.as_slice() else {
            return Err(CliError::fatal(format!(
                "missing or repeated {name} header from {url}"
            )));
        };
        let value = value
            .to_str()
            .map_err(|error| CliError::fatal(format!("invalid {name} header: {error}")))?;
        if !is_hex(value, len) {
            return Err(CliError::fatal(format!(
                "invalid {name} header from {url}: expected lowercase {len}-hex identity"
            )));
        }
        Ok(value.to_owned())
    };
    let build = header("X-Plotnik-Build", 64)?;
    let commit = header("X-Plotnik-Commit", 40)?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|error| CliError::fatal(format!("failed to fetch {url}: {error}")))?;
    Ok(Artifact {
        build,
        commit,
        bytes,
    })
}

fn cache_path(root: &Path, selection: &Selection) -> PathBuf {
    let name = match &selection.revision {
        Some(revision) => format!("{}@{revision}", selection.name),
        None => selection.name.clone(),
    };
    root.join(utf8_percent_encode(&name, CACHE_COMPONENT).to_string())
}

fn read_cache(dir: &Path) -> Result<Option<Artifacts>, CliError> {
    if !dir.try_exists().map_err(io_error)? {
        return Ok(None);
    }

    let json = fs::read(dir.join("grammar.json")).map_err(io_error)?;
    let wasm = fs::read(dir.join("parser.wasm")).map_err(io_error)?;
    artifacts(dir, &json, &wasm).map(Some)
}

fn write_cache(dir: &Path, json: Vec<u8>, wasm: Vec<u8>) -> Result<Artifacts, CliError> {
    let artifacts = artifacts(dir, &json, &wasm)?;

    let parent = dir.parent().expect("cache path has a language directory");
    fs::create_dir_all(parent).map_err(io_error)?;
    let pending = tempfile::tempdir_in(parent).map_err(io_error)?;
    fs::write(pending.path().join("grammar.json"), json).map_err(io_error)?;
    fs::write(pending.path().join("parser.wasm"), wasm).map_err(io_error)?;

    match fs::rename(pending.path(), dir) {
        Ok(()) => Ok(artifacts),
        Err(_) if dir.is_dir() => read_cache(dir)?
            .ok_or_else(|| CliError::fatal(format!("cache disappeared: '{}'", dir.display()))),
        Err(error) => Err(io_error(error)),
    }
}

fn artifacts(dir: &Path, json: &[u8], wasm: &[u8]) -> Result<Artifacts, CliError> {
    let source = dir.join("grammar.json").display().to_string();
    let grammar = grammar::from_bytes(json, &source)?;
    let parser = WasmParser::new(grammar.clone(), wasm)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    Ok(Artifacts { grammar, parser })
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn io_error(error: std::io::Error) -> CliError {
    CliError::fatal(format!("service artifact IO error: {error}"))
}

fn cache_dir(m: &ArgMatches) -> Result<PathBuf, CliError> {
    m.get_one::<PathBuf>("cache_dir")
        .cloned()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("plotnik").join("grammars")))
        .ok_or_else(|| CliError::fatal("OS cache directory unavailable; use --cache-dir"))
}

fn request(url: &str) -> Result<ureq::http::Response<ureq::Body>, CliError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .http_status_as_error(false)
        .https_only(true)
        .user_agent(concat!("plotnik/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    agent
        .get(url)
        .header("Cache-Control", "no-cache")
        .call()
        .map_err(|error| CliError::fatal(format!("failed to fetch {url}: {error}")))
}
