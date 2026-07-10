use std::process::ExitCode;

use plotnik_tests::conformance::{default_corpus_dir, generate_corpus};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("export-conformance: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let check = match args.next().as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => return Err(format!("unknown argument `{other}` (expected `--check`)")),
    };
    if let Some(extra) = args.next() {
        return Err(format!("unexpected extra argument `{extra}`"));
    }

    let corpus = generate_corpus()?;
    let directory = default_corpus_dir();
    if check {
        corpus.check(&directory)?;
        println!("{} conformance cases are current", corpus.case_count());
        return Ok(());
    }

    corpus.write(&directory)?;
    println!(
        "wrote {} conformance cases to {}",
        corpus.case_count(),
        directory.display()
    );
    Ok(())
}
