pub fn run() {
    let langs = plotnik_langs::all();
    println!("Supported languages ({}):", langs.len());
    for lang in langs {
        println!("  {}", lang.name());
    }
}

#[cfg(test)]
mod tests {
    use plotnik_langs::Lang;

    fn smoke_test(lang: Lang, source: &str, expected_root: &str) {
        let tree = lang.parse(source);
        let root = tree.root_node();
        assert_eq!(root.kind(), expected_root);
        assert!(!root.has_error());
    }

    #[test]
    #[cfg(feature = "lang-bash")]
    fn smoke_parse_bash() {
        smoke_test(plotnik_langs::bash(), "echo hello", "program");
    }

    #[test]
    #[cfg(feature = "lang-c")]
    fn smoke_parse_c() {
        smoke_test(
            plotnik_langs::c(),
            "int main() { return 0; }",
            "translation_unit",
        );
    }

    #[test]
    #[cfg(feature = "lang-cpp")]
    fn smoke_parse_cpp() {
        smoke_test(
            plotnik_langs::cpp(),
            "int main() { return 0; }",
            "translation_unit",
        );
    }

    #[test]
    #[cfg(feature = "lang-c-sharp")]
    fn smoke_parse_csharp() {
        smoke_test(plotnik_langs::csharp(), "class Foo { }", "compilation_unit");
    }

    #[test]
    #[cfg(feature = "lang-css")]
    fn smoke_parse_css() {
        smoke_test(plotnik_langs::css(), "body { color: red; }", "stylesheet");
    }

    #[test]
    #[cfg(feature = "lang-elixir")]
    fn smoke_parse_elixir() {
        smoke_test(plotnik_langs::elixir(), "defmodule Foo do end", "source");
    }

    #[test]
    #[cfg(feature = "lang-go")]
    fn smoke_parse_go() {
        smoke_test(plotnik_langs::go(), "package main", "source_file");
    }

    #[test]
    #[cfg(feature = "lang-haskell")]
    fn smoke_parse_haskell() {
        smoke_test(
            plotnik_langs::haskell(),
            "main = putStrLn \"hello\"",
            "haskell",
        );
    }

    #[test]
    #[cfg(feature = "lang-hcl")]
    fn smoke_parse_hcl() {
        smoke_test(
            plotnik_langs::hcl(),
            "resource \"aws_instance\" \"x\" {}",
            "config_file",
        );
    }

    #[test]
    #[cfg(feature = "lang-html")]
    fn smoke_parse_html() {
        smoke_test(plotnik_langs::html(), "<html></html>", "document");
    }

    #[test]
    #[cfg(feature = "lang-java")]
    fn smoke_parse_java() {
        smoke_test(plotnik_langs::java(), "class Foo {}", "program");
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn smoke_parse_javascript() {
        smoke_test(
            plotnik_langs::javascript(),
            "function hello() { return 42; }",
            "program",
        );
    }

    #[test]
    #[cfg(feature = "lang-json")]
    fn smoke_parse_json() {
        smoke_test(plotnik_langs::json(), r#"{"key": "value"}"#, "document");
    }

    #[test]
    #[cfg(feature = "lang-kotlin")]
    fn smoke_parse_kotlin() {
        smoke_test(plotnik_langs::kotlin(), "fun main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "lang-lua")]
    fn smoke_parse_lua() {
        smoke_test(plotnik_langs::lua(), "print('hello')", "chunk");
    }

    #[test]
    #[cfg(feature = "lang-nix")]
    fn smoke_parse_nix() {
        smoke_test(plotnik_langs::nix(), "{ x = 1; }", "source_code");
    }

    #[test]
    #[cfg(feature = "lang-php")]
    fn smoke_parse_php() {
        smoke_test(plotnik_langs::php(), "<?php echo 1;", "program");
    }

    #[test]
    #[cfg(feature = "lang-python")]
    fn smoke_parse_python() {
        smoke_test(
            plotnik_langs::python(),
            "def hello():\n    return 42",
            "module",
        );
    }

    #[test]
    #[cfg(feature = "lang-ruby")]
    fn smoke_parse_ruby() {
        smoke_test(plotnik_langs::ruby(), "def hello; end", "program");
    }

    #[test]
    #[cfg(feature = "lang-rust")]
    fn smoke_parse_rust() {
        smoke_test(plotnik_langs::rust(), "fn main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "lang-scala")]
    fn smoke_parse_scala() {
        smoke_test(plotnik_langs::scala(), "object Main {}", "compilation_unit");
    }

    #[test]
    #[cfg(feature = "lang-swift")]
    fn smoke_parse_swift() {
        smoke_test(plotnik_langs::swift(), "func main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "lang-typescript")]
    fn smoke_parse_typescript() {
        smoke_test(
            plotnik_langs::typescript(),
            "const x: number = 42;",
            "program",
        );
    }

    #[test]
    #[cfg(feature = "lang-tsx")]
    fn smoke_parse_tsx() {
        smoke_test(plotnik_langs::tsx(), "const x = <div />;", "program");
    }

    #[test]
    #[cfg(feature = "lang-yaml")]
    fn smoke_parse_yaml() {
        smoke_test(plotnik_langs::yaml(), "key: value", "stream");
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn lang_from_name() {
        assert_eq!(plotnik_langs::from_name("js").unwrap().name(), "javascript");
        assert_eq!(
            plotnik_langs::from_name("JavaScript").unwrap().name(),
            "javascript"
        );
        assert!(plotnik_langs::from_name("unknown").is_none());
    }

    #[test]
    #[cfg(feature = "lang-javascript")]
    fn lang_from_extension() {
        assert_eq!(plotnik_langs::from_ext("js").unwrap().name(), "javascript");
        assert_eq!(plotnik_langs::from_ext("mjs").unwrap().name(), "javascript");
    }
}
