mod cli;
mod commands;

use cli::{
    AstParams, CheckParams, DumpParams, ExecParams, InferParams, LangDumpParams, LangListParams,
    TraceParams, build_cli,
};

fn main() {
    let matches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("ast", m)) => {
            let params = AstParams::from_matches(m);
            commands::ast::run(params.into());
        }
        Some(("check", m)) => {
            let params = CheckParams::from_matches(m);
            commands::check::run(params.into());
        }
        Some(("dump", m)) => {
            let params = DumpParams::from_matches(m);
            commands::dump::run(params.into());
        }
        Some(("infer", m)) => {
            let params = InferParams::from_matches(m);
            commands::infer::run(params.into());
        }
        Some(("exec", m)) => {
            let params = ExecParams::from_matches(m);
            commands::exec::run(params.into());
        }
        Some(("trace", m)) => {
            let params = TraceParams::from_matches(m);
            commands::trace::run(params.into());
        }
        Some(("lang", m)) => match m.subcommand() {
            Some(("list", sub_m)) => {
                let _params = LangListParams::from_matches(sub_m);
                commands::lang::run_list();
            }
            Some(("dump", sub_m)) => {
                let params = LangDumpParams::from_matches(sub_m);
                commands::lang::run_dump(&params.lang);
            }
            _ => unreachable!("clap should have caught this"),
        },
        _ => unreachable!("clap should have caught this"),
    }
}
