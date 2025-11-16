pub fn run(topic: Option<&str>) {
    match topic {
        None => {
            println!("Available topics:");
            println!("  reference  - Query language reference");
            println!("  examples   - Example queries");
            println!();
            println!("Usage: plotnik docs <topic>");
        }
        Some("reference") => {
            println!("{}", include_str!("../../../../docs/REFERENCE.md"));
        }
        Some("examples") => {
            println!("(examples not yet written)");
        }
        Some(other) => {
            eprintln!("Unknown help topic: {}", other);
            eprintln!("Run 'plotnik docs' to see available topics");
            std::process::exit(1);
        }
    }
}
