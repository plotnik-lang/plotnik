use plotnik_langs::Lang;

pub fn run() {
    let langs = Lang::all();
    println!("Supported languages ({}):", langs.len());
    for lang in langs {
        println!("  {}", lang.name());
    }
}

#[cfg(test)]
mod tests {
    use plotnik_langs::Lang;

    fn smoke_test(lang: Lang, source: &str, expected_root: &str) {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();
        assert_eq!(root.kind(), expected_root);
        assert!(!root.has_error());
    }

    #[test]
    #[cfg(feature = "bash")]
    fn smoke_parse_bash() {
        smoke_test(Lang::Bash, "echo hello", "program");
    }

    #[test]
    #[cfg(feature = "c")]
    fn smoke_parse_c() {
        smoke_test(Lang::C, "int main() { return 0; }", "translation_unit");
    }

    #[test]
    #[cfg(feature = "cpp")]
    fn smoke_parse_cpp() {
        smoke_test(Lang::Cpp, "int main() { return 0; }", "translation_unit");
    }

    #[test]
    #[cfg(feature = "csharp")]
    fn smoke_parse_csharp() {
        smoke_test(Lang::CSharp, "class Foo { }", "compilation_unit");
    }

    #[test]
    #[cfg(feature = "css")]
    fn smoke_parse_css() {
        smoke_test(Lang::Css, "body { color: red; }", "stylesheet");
    }

    #[test]
    #[cfg(feature = "elixir")]
    fn smoke_parse_elixir() {
        smoke_test(Lang::Elixir, "defmodule Foo do end", "source");
    }

    #[test]
    #[cfg(feature = "go")]
    fn smoke_parse_go() {
        smoke_test(Lang::Go, "package main", "source_file");
    }

    #[test]
    #[cfg(feature = "haskell")]
    fn smoke_parse_haskell() {
        smoke_test(Lang::Haskell, "main = putStrLn \"hello\"", "haskell");
    }

    #[test]
    #[cfg(feature = "hcl")]
    fn smoke_parse_hcl() {
        smoke_test(
            Lang::Hcl,
            "resource \"aws_instance\" \"x\" {}",
            "config_file",
        );
    }

    #[test]
    #[cfg(feature = "html")]
    fn smoke_parse_html() {
        smoke_test(Lang::Html, "<html></html>", "document");
    }

    #[test]
    #[cfg(feature = "java")]
    fn smoke_parse_java() {
        smoke_test(Lang::Java, "class Foo {}", "program");
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn smoke_parse_javascript() {
        smoke_test(
            Lang::JavaScript,
            "function hello() { return 42; }",
            "program",
        );
    }

    #[test]
    #[cfg(feature = "json")]
    fn smoke_parse_json() {
        smoke_test(Lang::Json, r#"{"key": "value"}"#, "document");
    }

    #[test]
    #[cfg(feature = "kotlin")]
    fn smoke_parse_kotlin() {
        smoke_test(Lang::Kotlin, "fun main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "lua")]
    fn smoke_parse_lua() {
        smoke_test(Lang::Lua, "print('hello')", "chunk");
    }

    #[test]
    #[cfg(feature = "nix")]
    fn smoke_parse_nix() {
        smoke_test(Lang::Nix, "{ x = 1; }", "source_code");
    }

    #[test]
    #[cfg(feature = "php")]
    fn smoke_parse_php() {
        smoke_test(Lang::Php, "<?php echo 1;", "program");
    }

    #[test]
    #[cfg(feature = "python")]
    fn smoke_parse_python() {
        smoke_test(Lang::Python, "def hello():\n    return 42", "module");
    }

    #[test]
    #[cfg(feature = "ruby")]
    fn smoke_parse_ruby() {
        smoke_test(Lang::Ruby, "def hello; end", "program");
    }

    #[test]
    #[cfg(feature = "rust")]
    fn smoke_parse_rust() {
        smoke_test(Lang::Rust, "fn main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "scala")]
    fn smoke_parse_scala() {
        smoke_test(Lang::Scala, "object Main {}", "compilation_unit");
    }

    #[test]
    #[cfg(feature = "solidity")]
    fn smoke_parse_solidity() {
        smoke_test(Lang::Solidity, "contract Foo {}", "source_file");
    }

    #[test]
    #[cfg(feature = "swift")]
    fn smoke_parse_swift() {
        smoke_test(Lang::Swift, "func main() {}", "source_file");
    }

    #[test]
    #[cfg(feature = "typescript")]
    fn smoke_parse_typescript() {
        smoke_test(Lang::TypeScript, "const x: number = 42;", "program");
    }

    #[test]
    #[cfg(feature = "typescript")]
    fn smoke_parse_tsx() {
        smoke_test(Lang::Tsx, "const x = <div />;", "program");
    }

    #[test]
    #[cfg(feature = "yaml")]
    fn smoke_parse_yaml() {
        smoke_test(Lang::Yaml, "key: value", "stream");
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_name() {
        assert_eq!(Lang::from_name("js"), Some(Lang::JavaScript));
        assert_eq!(Lang::from_name("JavaScript"), Some(Lang::JavaScript));
        assert_eq!(Lang::from_name("unknown"), None);
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_extension() {
        assert_eq!(Lang::from_extension("js"), Some(Lang::JavaScript));
        assert_eq!(Lang::from_extension("mjs"), Some(Lang::JavaScript));
    }
}
