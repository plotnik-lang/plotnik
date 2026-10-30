use std::path::PathBuf;

use clap::{Arg, ArgAction, value_parser};

pub fn query_path_arg() -> Arg {
    Arg::new("query_path")
        .value_name("PATH")
        .value_parser(value_parser!(PathBuf))
        .required(true)
        .help("Query file, directory of .ptk files, or '-' for stdin")
}

pub const LANGUAGE_HELP: &str =
    "Language name or owner/repo[/grammar-directory], optionally followed by @revision";

pub fn lang_arg() -> Arg {
    Arg::new("lang")
        .short('l')
        .long("lang")
        .value_name("LANG[@REVISION]")
        .help(LANGUAGE_HELP)
}

pub fn cache_dir_arg() -> Arg {
    Arg::new("cache_dir")
        .long("cache-dir")
        .value_name("DIR")
        .value_parser(value_parser!(PathBuf))
        .help("Override the OS cache directory for grammar artifacts")
}

pub fn color_arg() -> Arg {
    Arg::new("color")
        .long("color")
        .value_name("WHEN")
        .default_value("auto")
        .value_parser(["auto", "always", "never"])
        .help("Colorize output")
}

pub fn include_anonymous_arg() -> Arg {
    Arg::new("include_anonymous")
        .long("include-anonymous")
        .action(ArgAction::SetTrue)
        .help("Include anonymous source-tree nodes such as literals and punctuation")
}

pub fn strict_arg() -> Arg {
    Arg::new("strict")
        .long("strict")
        .action(ArgAction::SetTrue)
        .help("Treat warnings as errors")
}

pub fn compact_arg() -> Arg {
    Arg::new("compact")
        .long("compact")
        .action(ArgAction::SetTrue)
        .help("Output compact JSON (default: pretty when stdout is a TTY)")
}

pub fn entry_arg() -> Arg {
    Arg::new("entry")
        .long("entry")
        .value_name("NAME")
        .help("Entrypoint name (definition to match from)")
}

pub fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .action(ArgAction::Count)
        .help("Trace detail (-v: named-node text and suppression, -vv: full text and applied effects)")
}
