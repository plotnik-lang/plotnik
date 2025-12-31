pub fn run() {
    let langs = plotnik_langs::all();
    println!("Supported languages ({}):", langs.len());
    for lang in langs {
        println!("  {}", lang.name());
    }
}
