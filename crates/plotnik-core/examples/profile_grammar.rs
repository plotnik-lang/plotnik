use std::env;
use std::fs;
#[cfg(plotnik_grammar_profile)]
use std::fs::OpenOptions;
use std::hint::black_box;
#[cfg(plotnik_grammar_profile)]
use std::io::{BufWriter, Write};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use plotnik_core::grammar::{Grammar, raw::RawGrammar};
#[cfg(plotnik_grammar_profile)]
use serde::Serialize;

#[derive(Debug)]
struct Args {
    grammar_jsons: Vec<PathBuf>,
    repeat: usize,
    warmup: usize,
    quiet: bool,
    profile_jsonl: Option<PathBuf>,
}

#[cfg(plotnik_grammar_profile)]
#[derive(Serialize)]
struct ProfileRecord {
    path: String,
    repeat_index: usize,
    elapsed_ms: f64,
    checksum: usize,
    profile: plotnik_core::grammar::profile::ProfileSnapshot,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse()?;
    let inputs = args
        .grammar_jsons
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map(|json| (path, json))
                .map_err(|error| format!("read {}: {error}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut warmup_grammars = Vec::with_capacity(args.warmup * inputs.len());
    for _ in 0..args.warmup {
        for (_, json) in &inputs {
            let grammar = parse_grammar(black_box(json.as_str()))?;
            consume_grammar(&grammar);
            warmup_grammars.push(grammar);
        }
    }
    black_box(&warmup_grammars);
    std::mem::forget(warmup_grammars);

    let started = Instant::now();
    let mut checksum = 0usize;
    let mut grammars = Vec::with_capacity(args.repeat * inputs.len());
    #[cfg(plotnik_grammar_profile)]
    let mut profile_writer = args
        .profile_jsonl
        .as_ref()
        .map(|path| {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .map(BufWriter::new)
                .map_err(|error| format!("open {}: {error}", path.display()))
        })
        .transpose()?;

    #[cfg(not(plotnik_grammar_profile))]
    if args.profile_jsonl.is_some() {
        return Err(
            "--profile-jsonl requires building with RUSTFLAGS='--cfg plotnik_grammar_profile'"
                .to_string(),
        );
    }

    for repeat_index in 0..args.repeat {
        #[cfg(not(plotnik_grammar_profile))]
        let _ = repeat_index;

        for (path, json) in &inputs {
            #[cfg(plotnik_grammar_profile)]
            if let Some(writer) = profile_writer.as_mut() {
                plotnik_core::grammar::profile::reset();
                let grammar_started = Instant::now();
                let grammar = parse_grammar(black_box(json.as_str()))?;
                let grammar_elapsed = grammar_started.elapsed();
                let profile = plotnik_core::grammar::profile::snapshot();
                let grammar_checksum = consume_grammar(&grammar);
                checksum = checksum.wrapping_add(grammar_checksum);
                grammars.push(grammar);

                let record = ProfileRecord {
                    path: path.display().to_string(),
                    repeat_index,
                    elapsed_ms: grammar_elapsed.as_secs_f64() * 1000.0,
                    checksum: grammar_checksum,
                    profile,
                };
                serde_json::to_writer(&mut *writer, &record)
                    .map_err(|error| format!("write profile record: {error}"))?;
                writeln!(writer).map_err(|error| format!("write profile record: {error}"))?;

                if !args.quiet {
                    println!("profiled: {} {:.3}ms", path.display(), record.elapsed_ms);
                }
                continue;
            }

            let grammar = parse_grammar(black_box(json.as_str()))?;
            checksum = checksum.wrapping_add(consume_grammar(&grammar));
            grammars.push(grammar);
            if !args.quiet {
                println!("parsed: {}", path.display());
            }
        }
    }
    let elapsed = started.elapsed();
    let total_ms = elapsed.as_secs_f64() * 1000.0;
    let avg_ms = total_ms / args.repeat as f64;

    if args.quiet {
        println!(
            "repeat={} total_ms={total_ms:.3} avg_ms={avg_ms:.3} checksum={checksum}",
            args.repeat
        );
    } else {
        println!("grammars: {}", args.grammar_jsons.len());
        println!("repeat: {}", args.repeat);
        println!("warmup: {}", args.warmup);
        println!("total_ms: {total_ms:.3}");
        println!("avg_ms: {avg_ms:.3}");
        println!("checksum: {checksum}");
    }

    black_box(&grammars);
    std::mem::forget(grammars);

    Ok(())
}

fn parse_grammar(json: &str) -> Result<Grammar, String> {
    let raw = RawGrammar::from_json(json).map_err(|error| error.to_string())?;
    Grammar::from_raw(&raw).map_err(|error| error.to_string())
}

fn consume_grammar(grammar: &Grammar) -> usize {
    let root = grammar.root().map(NonZeroU16::get).unwrap_or_default() as usize;
    let checksum = grammar.name().len() ^ root;
    black_box(&grammar);
    black_box(checksum);
    checksum
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = env::args().skip(1);
        let mut grammar_jsons = Vec::new();
        let mut repeat = 1usize;
        let mut warmup = 0usize;
        let mut quiet = false;
        let mut profile_jsonl = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Err(Self::usage()),
                "--repeat" => {
                    repeat = parse_usize("--repeat", args.next())?;
                }
                "--warmup" => {
                    warmup = parse_usize("--warmup", args.next())?;
                }
                "--paths-file" => {
                    let Some(path) = args.next() else {
                        return Err("--paths-file expects a value".to_string());
                    };
                    let path = PathBuf::from(path);
                    let contents = fs::read_to_string(&path)
                        .map_err(|error| format!("read {}: {error}", path.display()))?;
                    grammar_jsons.extend(
                        contents
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(PathBuf::from),
                    );
                }
                "-q" | "--quiet" => {
                    quiet = true;
                }
                "--profile-jsonl" => {
                    let Some(path) = args.next() else {
                        return Err("--profile-jsonl expects a value".to_string());
                    };
                    profile_jsonl = Some(PathBuf::from(path));
                }
                _ if arg.starts_with('-') => {
                    return Err(format!("unknown option {arg:?}\n\n{}", Self::usage()));
                }
                _ => {
                    grammar_jsons.push(PathBuf::from(arg));
                }
            }
        }

        if grammar_jsons.is_empty() {
            return Err(Self::usage());
        }
        if repeat == 0 {
            return Err("--repeat must be greater than 0".to_string());
        }

        Ok(Self {
            grammar_jsons,
            repeat,
            warmup,
            quiet,
            profile_jsonl,
        })
    }

    fn usage() -> String {
        "usage: profile_grammar <grammar.json>... [--paths-file FILE] [--repeat N] [--warmup N] [--quiet] [--profile-jsonl FILE]".to_string()
    }
}

fn parse_usize(name: &str, value: Option<String>) -> Result<usize, String> {
    let Some(value) = value else {
        return Err(format!("{name} expects a value"));
    };
    value
        .parse()
        .map_err(|error| format!("invalid {name} value {value:?}: {error}"))
}
