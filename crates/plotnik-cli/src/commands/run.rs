//! Execute a query and output JSON result.

use std::path::PathBuf;

use plotnik_lib::{
    Colors, NoopTracer, RuntimeError, RuntimeLimitSpec, VM, extract_result_provenance,
    materialize_verified,
};

use super::run_common::{self, ExecPlan, ExecRequest};
use super::runtime_report::render_runtime_error;
use crate::error::{CliError, CliResult};

pub struct RunArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub entry: Option<String>,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
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
        inspection: args.json,
    })?;

    let vm = VM::builder(&source_code, &tree).limits(args.limits).build();
    if args.json {
        let mut tracer = NoopTracer;
        let (result, stats) = vm.execute_with_stats(&module, &entrypoint, &mut tracer);
        let effects = match result {
            Ok(effects) => effects,
            Err(RuntimeError::NoMatch) => {
                println!(
                    "{}",
                    serde_json::json!({
                        "value": null,
                        "error": "no match",
                        "stats": stats,
                    })
                );
                return Err(CliError::No);
            }
            Err(e) => {
                eprintln!("{}", render_runtime_error(&e, true));
                return Err(CliError::FatalRendered);
            }
        };

        let colors = Colors::new(false);
        let value = materialize_verified(
            &source_code,
            &module,
            &entrypoint,
            effects.as_slice(),
            colors,
        );
        let result_provenance = (!module.spans().is_empty())
            .then(|| extract_result_provenance(effects.as_slice(), &module));
        println!(
            "{}",
            serde_json::json!({
                "value": value,
                "inspection": result_provenance,
                "stats": stats,
            })
        );
        return Ok(());
    }

    let effects = match vm.execute(&module, &entrypoint) {
        Ok(effects) => effects,
        Err(RuntimeError::NoMatch) => {
            // Zero matches must never be silent
            eprintln!("no match");
            return Err(CliError::No);
        }
        Err(e) => {
            eprintln!("{}", render_runtime_error(&e, args.json));
            return Err(CliError::FatalRendered);
        }
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
