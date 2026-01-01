use std::fs;
use std::io::{self, Read};
use std::path::Path;

use plotnik_lib::SourceMap;

pub fn load_query_source(
    query_path: Option<&Path>,
    query_text: Option<&str>,
) -> Result<SourceMap, String> {
    if let Some(text) = query_text {
        let mut map = SourceMap::new();
        map.add_one_liner(text);
        return Ok(map);
    }

    if let Some(path) = query_path {
        if path.as_os_str() == "-" {
            return load_stdin();
        }
        if path.is_dir() {
            return load_workspace(path);
        }
        return load_file(path);
    }

    Err("query is required: use positional argument, -q/--query, or --query-file".to_string())
}

fn load_stdin() -> Result<SourceMap, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("failed to read stdin: {}", e))?;
    let mut map = SourceMap::new();
    map.add_stdin(&buf);
    Ok(map)
}

fn load_file(path: &Path) -> Result<SourceMap, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;
    let mut map = SourceMap::new();
    map.add_file(&path.to_string_lossy(), &content);
    Ok(map)
}

fn load_workspace(dir: &Path) -> Result<SourceMap, String> {
    let mut map = SourceMap::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory '{}': {}", dir.display(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "ptk")
                .unwrap_or(false)
        })
        .collect();

    if entries.is_empty() {
        return Err(format!(
            "no .ptk files found in workspace '{}'",
            dir.display()
        ));
    }

    // Sort for deterministic ordering
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read '{}': {}", path.display(), e))?;
        map.add_file(&path.to_string_lossy(), &content);
    }

    Ok(map)
}
