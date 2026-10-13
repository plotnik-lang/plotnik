use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

mod corpus;
mod generate;
mod process;
mod rust;

struct Args {
    target: String,
    filter: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let started = Instant::now();
    let args = parse_args()?;
    if args.target != "rust" {
        return Err(format!(
            "unsupported codegen test target `{}` (available: rust)",
            args.target
        ));
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus = corpus::discover(manifest_dir, args.filter.as_deref())?;
    if corpus.cases.is_empty() {
        return Err("codegen corpus selection contains no runnable snapshots".to_string());
    }

    rust::generate(manifest_dir, &corpus.cases)?;
    eprintln!(
        "generated {} Rust tests from {} snapshots ({} diagnostic-only skipped) in {:.2?}",
        corpus.cases.len(),
        corpus.selected,
        corpus.skipped,
        started.elapsed()
    );
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let target = values.next().ok_or_else(usage)?;
    let mut filter = None;

    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--filter" => {
                filter = Some(
                    values
                        .next()
                        .ok_or_else(|| "`--filter` requires a value".to_string())?,
                );
            }
            "-h" | "--help" => return Err(usage()),
            unknown => return Err(format!("unknown argument `{unknown}`\n{}", usage())),
        }
    }

    Ok(Args { target, filter })
}

fn usage() -> String {
    "usage: plotnik-codegen-tests rust [--filter <TEXT>]".to_string()
}
