use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::QueryBuilder;
use plotnik_lib::bytecode::{Module, dump};

use super::lang_resolver::require_lang;
use super::query_loader::load_query_source;

pub struct DumpArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub color: bool,
}

pub fn run(args: DumpArgs) {
    let source_map = match load_query_source(args.query_path.as_deref(), args.query_text.as_deref())
    {
        Ok(map) => map,
        Err(msg) => {
            eprintln!("error: {}", msg);
            std::process::exit(1);
        }
    };

    if source_map.is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }

    // Parse and analyze
    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze(),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let lang = require_lang(args.lang.as_deref(), args.query_path.as_deref(), "dump");

    let linked = query.link(&lang);
    if !linked.is_valid() {
        eprint!(
            "{}",
            linked
                .diagnostics()
                .render_colored(linked.source_map(), args.color)
        );
        std::process::exit(1);
    }
    let bytecode = linked.emit().expect("bytecode emission failed");

    let module = Module::load(&bytecode).expect("module loading failed");
    let colors = Colors::new(args.color);
    print!("{}", dump(&module, colors));
}
