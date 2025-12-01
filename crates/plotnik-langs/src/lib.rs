use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Lang {
    #[cfg(feature = "bash")]
    Bash,
    #[cfg(feature = "c")]
    C,
    #[cfg(feature = "cpp")]
    Cpp,
    #[cfg(feature = "csharp")]
    CSharp,
    #[cfg(feature = "css")]
    Css,
    #[cfg(feature = "elixir")]
    Elixir,
    #[cfg(feature = "go")]
    Go,
    #[cfg(feature = "haskell")]
    Haskell,
    #[cfg(feature = "hcl")]
    Hcl,
    #[cfg(feature = "html")]
    Html,
    #[cfg(feature = "java")]
    Java,
    #[cfg(feature = "javascript")]
    JavaScript,
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "kotlin")]
    Kotlin,
    #[cfg(feature = "lua")]
    Lua,
    #[cfg(feature = "nix")]
    Nix,
    #[cfg(feature = "php")]
    Php,
    #[cfg(feature = "python")]
    Python,
    #[cfg(feature = "ruby")]
    Ruby,
    #[cfg(feature = "rust")]
    Rust,
    #[cfg(feature = "scala")]
    Scala,
    #[cfg(feature = "solidity")]
    Solidity,
    #[cfg(feature = "swift")]
    Swift,
    #[cfg(feature = "typescript")]
    TypeScript,
    #[cfg(feature = "typescript")]
    Tsx,
    #[cfg(feature = "yaml")]
    Yaml,
}

impl Lang {
    pub fn language(&self) -> Language {
        match self {
            #[cfg(feature = "bash")]
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            #[cfg(feature = "c")]
            Self::C => tree_sitter_c::LANGUAGE.into(),
            #[cfg(feature = "cpp")]
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            #[cfg(feature = "csharp")]
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            #[cfg(feature = "css")]
            Self::Css => tree_sitter_css::LANGUAGE.into(),
            #[cfg(feature = "elixir")]
            Self::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            #[cfg(feature = "go")]
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            #[cfg(feature = "haskell")]
            Self::Haskell => tree_sitter_haskell::LANGUAGE.into(),
            #[cfg(feature = "hcl")]
            Self::Hcl => tree_sitter_hcl::LANGUAGE.into(),
            #[cfg(feature = "html")]
            Self::Html => tree_sitter_html::LANGUAGE.into(),
            #[cfg(feature = "java")]
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            #[cfg(feature = "javascript")]
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            #[cfg(feature = "json")]
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            #[cfg(feature = "kotlin")]
            Self::Kotlin => tree_sitter_kotlin::LANGUAGE.into(),
            #[cfg(feature = "lua")]
            Self::Lua => tree_sitter_lua::LANGUAGE.into(),
            #[cfg(feature = "nix")]
            Self::Nix => tree_sitter_nix::LANGUAGE.into(),
            #[cfg(feature = "php")]
            Self::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            #[cfg(feature = "python")]
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            #[cfg(feature = "ruby")]
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            #[cfg(feature = "rust")]
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            #[cfg(feature = "scala")]
            Self::Scala => tree_sitter_scala::LANGUAGE.into(),
            #[cfg(feature = "solidity")]
            Self::Solidity => tree_sitter_solidity::LANGUAGE.into(),
            #[cfg(feature = "swift")]
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
            #[cfg(feature = "typescript")]
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            #[cfg(feature = "typescript")]
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            #[cfg(feature = "yaml")]
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            #[allow(unreachable_patterns)]
            _ => unreachable!("no languages enabled"),
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            #[cfg(feature = "bash")]
            "bash" | "sh" | "shell" => Some(Self::Bash),
            #[cfg(feature = "c")]
            "c" => Some(Self::C),
            #[cfg(feature = "cpp")]
            "cpp" | "c++" | "cxx" | "cc" => Some(Self::Cpp),
            #[cfg(feature = "csharp")]
            "csharp" | "c#" | "cs" => Some(Self::CSharp),
            #[cfg(feature = "css")]
            "css" => Some(Self::Css),
            #[cfg(feature = "elixir")]
            "elixir" | "ex" => Some(Self::Elixir),
            #[cfg(feature = "go")]
            "go" | "golang" => Some(Self::Go),
            #[cfg(feature = "haskell")]
            "haskell" | "hs" => Some(Self::Haskell),
            #[cfg(feature = "hcl")]
            "hcl" | "terraform" | "tf" => Some(Self::Hcl),
            #[cfg(feature = "html")]
            "html" => Some(Self::Html),
            #[cfg(feature = "java")]
            "java" => Some(Self::Java),
            #[cfg(feature = "javascript")]
            "javascript" | "js" | "jsx" => Some(Self::JavaScript),
            #[cfg(feature = "json")]
            "json" => Some(Self::Json),
            #[cfg(feature = "kotlin")]
            "kotlin" | "kt" => Some(Self::Kotlin),
            #[cfg(feature = "lua")]
            "lua" => Some(Self::Lua),
            #[cfg(feature = "nix")]
            "nix" => Some(Self::Nix),
            #[cfg(feature = "php")]
            "php" => Some(Self::Php),
            #[cfg(feature = "python")]
            "python" | "py" => Some(Self::Python),
            #[cfg(feature = "ruby")]
            "ruby" | "rb" => Some(Self::Ruby),
            #[cfg(feature = "rust")]
            "rust" | "rs" => Some(Self::Rust),
            #[cfg(feature = "scala")]
            "scala" => Some(Self::Scala),
            #[cfg(feature = "solidity")]
            "solidity" | "sol" => Some(Self::Solidity),
            #[cfg(feature = "swift")]
            "swift" => Some(Self::Swift),
            #[cfg(feature = "typescript")]
            "typescript" | "ts" => Some(Self::TypeScript),
            #[cfg(feature = "typescript")]
            "tsx" => Some(Self::Tsx),
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            #[cfg(feature = "bash")]
            "sh" | "bash" | "zsh" => Some(Self::Bash),
            #[cfg(feature = "c")]
            "c" => Some(Self::C),
            #[cfg(feature = "cpp")]
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "h++" | "c++" => Some(Self::Cpp),
            #[cfg(feature = "csharp")]
            "cs" => Some(Self::CSharp),
            #[cfg(feature = "css")]
            "css" => Some(Self::Css),
            #[cfg(feature = "elixir")]
            "ex" | "exs" => Some(Self::Elixir),
            #[cfg(feature = "go")]
            "go" => Some(Self::Go),
            #[cfg(feature = "haskell")]
            "hs" | "lhs" => Some(Self::Haskell),
            #[cfg(feature = "hcl")]
            "hcl" | "tf" | "tfvars" => Some(Self::Hcl),
            #[cfg(feature = "html")]
            "html" | "htm" => Some(Self::Html),
            #[cfg(feature = "java")]
            "java" => Some(Self::Java),
            #[cfg(feature = "javascript")]
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            #[cfg(feature = "json")]
            "json" => Some(Self::Json),
            #[cfg(feature = "kotlin")]
            "kt" | "kts" => Some(Self::Kotlin),
            #[cfg(feature = "lua")]
            "lua" => Some(Self::Lua),
            #[cfg(feature = "nix")]
            "nix" => Some(Self::Nix),
            #[cfg(feature = "php")]
            "php" => Some(Self::Php),
            #[cfg(feature = "python")]
            "py" | "pyi" | "pyw" => Some(Self::Python),
            #[cfg(feature = "ruby")]
            "rb" | "rake" | "gemspec" => Some(Self::Ruby),
            #[cfg(feature = "rust")]
            "rs" => Some(Self::Rust),
            #[cfg(feature = "scala")]
            "scala" | "sc" => Some(Self::Scala),
            #[cfg(feature = "solidity")]
            "sol" => Some(Self::Solidity),
            #[cfg(feature = "swift")]
            "swift" => Some(Self::Swift),
            #[cfg(feature = "typescript")]
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            #[cfg(feature = "typescript")]
            "tsx" => Some(Self::Tsx),
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => Some(Self::Yaml),
            // .h is ambiguous (C or C++), defaulting to C
            #[cfg(feature = "c")]
            "h" => Some(Self::C),
            _ => None,
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            #[cfg(feature = "bash")]
            Self::Bash,
            #[cfg(feature = "c")]
            Self::C,
            #[cfg(feature = "cpp")]
            Self::Cpp,
            #[cfg(feature = "csharp")]
            Self::CSharp,
            #[cfg(feature = "css")]
            Self::Css,
            #[cfg(feature = "elixir")]
            Self::Elixir,
            #[cfg(feature = "go")]
            Self::Go,
            #[cfg(feature = "haskell")]
            Self::Haskell,
            #[cfg(feature = "hcl")]
            Self::Hcl,
            #[cfg(feature = "html")]
            Self::Html,
            #[cfg(feature = "java")]
            Self::Java,
            #[cfg(feature = "javascript")]
            Self::JavaScript,
            #[cfg(feature = "json")]
            Self::Json,
            #[cfg(feature = "kotlin")]
            Self::Kotlin,
            #[cfg(feature = "lua")]
            Self::Lua,
            #[cfg(feature = "nix")]
            Self::Nix,
            #[cfg(feature = "php")]
            Self::Php,
            #[cfg(feature = "python")]
            Self::Python,
            #[cfg(feature = "ruby")]
            Self::Ruby,
            #[cfg(feature = "rust")]
            Self::Rust,
            #[cfg(feature = "scala")]
            Self::Scala,
            #[cfg(feature = "solidity")]
            Self::Solidity,
            #[cfg(feature = "swift")]
            Self::Swift,
            #[cfg(feature = "typescript")]
            Self::TypeScript,
            #[cfg(feature = "typescript")]
            Self::Tsx,
            #[cfg(feature = "yaml")]
            Self::Yaml,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "bash")]
            Self::Bash => "bash",
            #[cfg(feature = "c")]
            Self::C => "c",
            #[cfg(feature = "cpp")]
            Self::Cpp => "cpp",
            #[cfg(feature = "csharp")]
            Self::CSharp => "c_sharp",
            #[cfg(feature = "css")]
            Self::Css => "css",
            #[cfg(feature = "elixir")]
            Self::Elixir => "elixir",
            #[cfg(feature = "go")]
            Self::Go => "go",
            #[cfg(feature = "haskell")]
            Self::Haskell => "haskell",
            #[cfg(feature = "hcl")]
            Self::Hcl => "hcl",
            #[cfg(feature = "html")]
            Self::Html => "html",
            #[cfg(feature = "java")]
            Self::Java => "java",
            #[cfg(feature = "javascript")]
            Self::JavaScript => "javascript",
            #[cfg(feature = "json")]
            Self::Json => "json",
            #[cfg(feature = "kotlin")]
            Self::Kotlin => "kotlin",
            #[cfg(feature = "lua")]
            Self::Lua => "lua",
            #[cfg(feature = "nix")]
            Self::Nix => "nix",
            #[cfg(feature = "php")]
            Self::Php => "php",
            #[cfg(feature = "python")]
            Self::Python => "python",
            #[cfg(feature = "ruby")]
            Self::Ruby => "ruby",
            #[cfg(feature = "rust")]
            Self::Rust => "rust",
            #[cfg(feature = "scala")]
            Self::Scala => "scala",
            #[cfg(feature = "solidity")]
            Self::Solidity => "solidity",
            #[cfg(feature = "swift")]
            Self::Swift => "swift",
            #[cfg(feature = "typescript")]
            Self::TypeScript => "typescript",
            #[cfg(feature = "typescript")]
            Self::Tsx => "tsx",
            #[cfg(feature = "yaml")]
            Self::Yaml => "yaml",
            #[allow(unreachable_patterns)]
            _ => unreachable!("no languages enabled"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
