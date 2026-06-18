//! Execute a query and output JSON result.

use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::engine::{RuntimeError, VM, materialize_verified};

use super::run_common::{self, ExecPlan, ExecRequest};
use crate::error::{CliError, CliResult};

pub struct RunArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub entry: Option<String>,
    pub color: bool,
}

pub fn run(args: RunArgs) -> CliResult {
    let ExecPlan {
        module,
        entrypoint,
        tree,
        source_code,
    } = run_common::plan_exec(ExecRequest {
        query_path: args.query_path.as_deref(),
        query_text: args.query_text.as_deref(),
        source_path: args.source_path.as_deref(),
        source_text: args.source_text.as_deref(),
        lang: args.lang.as_deref(),
        entry: args.entry.as_deref(),
        color: args.color,
    })?;

    let vm = VM::builder(&source_code, &tree).build();
    let effects = match vm.execute(&module, 0, &entrypoint) {
        Ok(effects) => effects,
        Err(RuntimeError::NoMatch) => {
            // Zero matches must never be silent
            eprintln!("no match");
            return Err(CliError::No);
        }
        Err(e) => return Err(CliError::fatal(format!("runtime error: {}", e))),
    };

    let colors = Colors::new(args.color);
    let value = materialize_verified(
        &source_code,
        &module,
        &entrypoint,
        effects.as_slice(),
        colors,
    );

    let output = value.format(args.pretty, colors);
    println!("{}", output);

    Ok(())
}
