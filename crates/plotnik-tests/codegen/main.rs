use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

mod corpus;
mod generate;
mod process;
mod rust;

struct Args {
    target: String,
    plotnik: PathBuf,
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
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("plotnik-tests must live under <repo>/crates");
    let plotnik = absolute_from(repo_root, &args.plotnik);
    if !plotnik.is_file() {
        return Err(format!(
            "plotnik executable does not exist at {}",
            plotnik.display()
        ));
    }

    let discovery_started = Instant::now();
    let corpus = corpus::discover(manifest_dir, args.filter.as_deref())?;
    eprintln!(
        "codegen corpus: {} selected, {} runnable, {} skipped in {:.2?}",
        corpus.selected,
        corpus.cases.len(),
        corpus.skipped,
        discovery_started.elapsed()
    );
    if corpus.cases.is_empty() {
        return Err("codegen corpus selection contains no runnable fixtures".to_string());
    }

    rust::generate(manifest_dir, &plotnik, &corpus.cases)?;
    eprintln!("generated Rust corpus in {:.2?}", started.elapsed());
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let target = values.next().ok_or_else(usage)?;
    let mut plotnik = None;
    let mut filter = None;

    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--plotnik" => {
                plotnik = Some(PathBuf::from(
                    values
                        .next()
                        .ok_or_else(|| "`--plotnik` requires a path".to_string())?,
                ));
            }
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

    Ok(Args {
        target,
        plotnik: plotnik.ok_or_else(|| "missing required `--plotnik <PATH>`".to_string())?,
        filter,
    })
}

fn usage() -> String {
    "usage: plotnik-codegen-tests rust --plotnik <PATH> [--filter <TEXT>]".to_string()
}

fn absolute_from(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    root.join(path)
}
