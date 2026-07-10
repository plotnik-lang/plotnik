use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;
use plotnik_lib::GrammarIdentity;
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use tree_sitter::{Language, Parser, Tree};

#[derive(Debug)]
pub struct Lang {
    name: &'static str,
    aliases: &'static [&'static str],
    #[allow(dead_code)]
    extensions: &'static [&'static str],
    ts_language: Language,
    raw_json_gz: &'static [u8],
    source: &'static str,
    raw_json: OnceLock<String>,
    raw: OnceLock<RawGrammar>,
    grammar: OnceLock<Grammar>,
}

impl Lang {
    #[allow(dead_code)]
    fn new(
        name: &'static str,
        aliases: &'static [&'static str],
        extensions: &'static [&'static str],
        ts_language: Language,
        raw_json_gz: &'static [u8],
        source: &'static str,
    ) -> Self {
        Self {
            name,
            aliases,
            extensions,
            ts_language,
            raw_json_gz,
            source,
            raw_json: OnceLock::new(),
            raw: OnceLock::new(),
            grammar: OnceLock::new(),
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn aliases(&self) -> &[&'static str] {
        self.aliases
    }

    #[allow(dead_code)]
    pub fn extensions(&self) -> &[&'static str] {
        self.extensions
    }

    pub fn raw(&self) -> &RawGrammar {
        self.raw.get_or_init(|| {
            RawGrammar::from_json(self.grammar_json()).expect("invalid embedded grammar JSON")
        })
    }

    pub fn grammar_json(&self) -> &str {
        self.raw_json
            .get_or_init(|| gunzip(self.raw_json_gz).expect("invalid embedded grammar gzip"))
    }

    pub fn grammar(&self) -> &Grammar {
        self.grammar.get_or_init(|| {
            let grammar = Grammar::from_raw(self.raw()).expect("invalid embedded grammar metadata");
            let identity = GrammarIdentity::from_json_bytes(
                grammar.name(),
                self.grammar_json().as_bytes(),
                self.source,
            );
            grammar.with_identity(identity)
        })
    }

    #[cfg(test)]
    pub fn ts_language(&self) -> &Language {
        &self.ts_language
    }

    pub fn parse_source(&self, source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }
}

fn gunzip(bytes: &[u8]) -> std::io::Result<String> {
    let mut decoder = GzDecoder::new(bytes);
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;
    Ok(json)
}

macro_rules! define_langs {
    (
        $(
            $fn_name:ident => {
                feature: $feature:literal,
                name: $name:literal,
                ts_lang: $ts_lang:expr,
                env_suffix: $env_suffix:literal,
                names: [$($alias:literal),* $(,)?],
                extensions: [$($ext:literal),* $(,)?] $(,)?
            }
        ),* $(,)?
    ) => {
        $(
            #[cfg(feature = $feature)]
            pub fn $fn_name() -> &'static Lang {
                static LANGUAGE: std::sync::LazyLock<Lang> =
                    std::sync::LazyLock::new(|| {
                        Lang::new(
                            $name,
                            &[$($alias),*],
                            &[$($ext),*],
                            $ts_lang.into(),
                            include_bytes!(env!(concat!("PLOTNIK_GRAMMAR_JSON_GZ_", $env_suffix))),
                            env!(concat!("PLOTNIK_GRAMMAR_SOURCE_", $env_suffix)),
                        )
                    });

                &LANGUAGE
            }
        )*

        pub fn from_name(s: &str) -> Option<&'static Lang> {
            match s.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($alias)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        #[allow(unused_variables)]
        pub fn from_ext(ext: &str) -> Option<&'static Lang> {
            $(
                #[cfg(feature = $feature)]
                {
                    let lang = $fn_name();
                    if lang
                        .extensions()
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(ext))
                    {
                        return Some(lang);
                    }
                }
            )*

            None
        }

        pub fn all() -> Vec<&'static Lang> {
            vec![
                $(
                    #[cfg(feature = $feature)]
                    $fn_name(),
                )*
            ]
        }

        #[cfg(test)]
        pub fn enabled_language_names() -> Vec<&'static str> {
            vec![
                $(
                    #[cfg(feature = $feature)]
                    $name,
                )*
            ]
        }
    };
}

define_langs! {
    ada => {
        feature: "lang-ada",
        name: "ada",
        ts_lang: arborium_ada::language(),
        env_suffix: "ADA",
        names: ["ada"],
        extensions: ["ada", "adb", "ads"],
    },
    agda => {
        feature: "lang-agda",
        name: "agda",
        ts_lang: arborium_agda::language(),
        env_suffix: "AGDA",
        names: ["agda"],
        extensions: ["agda"],
    },
    asciidoc => {
        feature: "lang-asciidoc",
        name: "asciidoc",
        ts_lang: arborium_asciidoc::language(),
        env_suffix: "ASCIIDOC",
        names: ["asciidoc", "adoc"],
        extensions: ["adoc", "asciidoc", "asc"],
    },
    asm => {
        feature: "lang-asm",
        name: "asm",
        ts_lang: arborium_asm::language(),
        env_suffix: "ASM",
        names: ["asm", "assembly"],
        extensions: ["asm", "s"],
    },
    awk => {
        feature: "lang-awk",
        name: "awk",
        ts_lang: arborium_awk::language(),
        env_suffix: "AWK",
        names: ["awk", "gawk", "mawk", "nawk"],
        extensions: ["awk"],
    },
    bash => {
        feature: "lang-bash",
        name: "bash",
        ts_lang: arborium_bash::language(),
        env_suffix: "BASH",
        names: ["bash", "sh", "shell"],
        extensions: ["sh", "bash"],
    },
    batch => {
        feature: "lang-batch",
        name: "batch",
        ts_lang: arborium_batch::language(),
        env_suffix: "BATCH",
        names: ["batch", "bat", "cmd"],
        extensions: ["bat", "cmd"],
    },
    c => {
        feature: "lang-c",
        name: "c",
        ts_lang: arborium_c::language(),
        env_suffix: "C",
        names: ["c"],
        extensions: ["c", "h"],
    },
    caddy => {
        feature: "lang-caddy",
        name: "caddy",
        ts_lang: arborium_caddy::language(),
        env_suffix: "CADDY",
        names: ["caddy", "caddyfile"],
        extensions: ["caddyfile"],
    },
    capnp => {
        feature: "lang-capnp",
        name: "capnp",
        ts_lang: arborium_capnp::language(),
        env_suffix: "CAPNP",
        names: ["capnp", "capnproto"],
        extensions: ["capnp"],
    },
    cedar => {
        feature: "lang-cedar",
        name: "cedar",
        ts_lang: arborium_cedar::language(),
        env_suffix: "CEDAR",
        names: ["cedar"],
        extensions: ["cedar"],
    },
    cedarschema => {
        feature: "lang-cedarschema",
        name: "cedarschema",
        ts_lang: arborium_cedarschema::language(),
        env_suffix: "CEDARSCHEMA",
        names: ["cedarschema"],
        extensions: ["cedarschema"],
    },
    clojure => {
        feature: "lang-clojure",
        name: "clojure",
        ts_lang: arborium_clojure::language(),
        env_suffix: "CLOJURE",
        names: ["clojure", "clj"],
        extensions: ["clj", "cljs", "cljc", "edn"],
    },
    cmake => {
        feature: "lang-cmake",
        name: "cmake",
        ts_lang: arborium_cmake::language(),
        env_suffix: "CMAKE",
        names: ["cmake"],
        extensions: ["cmake"],
    },
    cobol => {
        feature: "lang-cobol",
        name: "cobol",
        ts_lang: arborium_cobol::language(),
        env_suffix: "COBOL",
        names: ["cobol", "cob"],
        extensions: ["cob", "cbl", "cpy"],
    },
    commonlisp => {
        feature: "lang-commonlisp",
        name: "commonlisp",
        ts_lang: arborium_commonlisp::language(),
        env_suffix: "COMMONLISP",
        names: ["commonlisp", "common-lisp", "lisp", "cl"],
        extensions: ["lisp", "lsp", "cl"],
    },
    cpp => {
        feature: "lang-cpp",
        name: "cpp",
        ts_lang: arborium_cpp::language(),
        env_suffix: "CPP",
        names: ["cpp", "c++", "cxx", "cc"],
        extensions: ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h++", "c++"],
    },
    csharp => {
        feature: "lang-c-sharp",
        name: "c_sharp",
        ts_lang: arborium_c_sharp::language(),
        env_suffix: "C_SHARP",
        names: ["csharp", "c#", "cs", "c_sharp"],
        extensions: ["cs"],
    },
    css => {
        feature: "lang-css",
        name: "css",
        ts_lang: arborium_css::language(),
        env_suffix: "CSS",
        names: ["css"],
        extensions: ["css"],
    },
    d => {
        feature: "lang-d",
        name: "d",
        ts_lang: arborium_d::language(),
        env_suffix: "D",
        names: ["d", "dlang"],
        extensions: ["d"],
    },
    dart => {
        feature: "lang-dart",
        name: "dart",
        ts_lang: arborium_dart::language(),
        env_suffix: "DART",
        names: ["dart"],
        extensions: ["dart"],
    },
    devicetree => {
        feature: "lang-devicetree",
        name: "devicetree",
        ts_lang: arborium_devicetree::language(),
        env_suffix: "DEVICETREE",
        names: ["devicetree", "dts"],
        extensions: ["dts", "dtsi"],
    },
    diff => {
        feature: "lang-diff",
        name: "diff",
        ts_lang: arborium_diff::language(),
        env_suffix: "DIFF",
        names: ["diff", "patch"],
        extensions: ["diff", "patch"],
    },
    dockerfile => {
        feature: "lang-dockerfile",
        name: "dockerfile",
        ts_lang: arborium_dockerfile::language(),
        env_suffix: "DOCKERFILE",
        names: ["dockerfile", "docker"],
        extensions: ["dockerfile"],
    },
    dot => {
        feature: "lang-dot",
        name: "dot",
        ts_lang: arborium_dot::language(),
        env_suffix: "DOT",
        names: ["dot", "graphviz"],
        extensions: ["dot", "gv"],
    },
    elisp => {
        feature: "lang-elisp",
        name: "elisp",
        ts_lang: arborium_elisp::language(),
        env_suffix: "ELISP",
        names: ["elisp", "emacs-lisp"],
        extensions: ["el"],
    },
    elixir => {
        feature: "lang-elixir",
        name: "elixir",
        ts_lang: arborium_elixir::language(),
        env_suffix: "ELIXIR",
        names: ["elixir", "ex"],
        extensions: ["ex", "exs"],
    },
    elm => {
        feature: "lang-elm",
        name: "elm",
        ts_lang: arborium_elm::language(),
        env_suffix: "ELM",
        names: ["elm"],
        extensions: ["elm"],
    },
    erlang => {
        feature: "lang-erlang",
        name: "erlang",
        ts_lang: arborium_erlang::language(),
        env_suffix: "ERLANG",
        names: ["erlang", "erl"],
        extensions: ["erl", "hrl"],
    },
    fish => {
        feature: "lang-fish",
        name: "fish",
        ts_lang: arborium_fish::language(),
        env_suffix: "FISH",
        names: ["fish"],
        extensions: ["fish"],
    },
    fsharp => {
        feature: "lang-fsharp",
        name: "fsharp",
        ts_lang: arborium_fsharp::language(),
        env_suffix: "FSHARP",
        names: ["fsharp", "f#", "fs"],
        extensions: ["fs", "fsi", "fsx"],
    },
    gitattributes => {
        feature: "lang-gitattributes",
        name: "gitattributes",
        ts_lang: arborium_gitattributes::language(),
        env_suffix: "GITATTRIBUTES",
        names: ["gitattributes", "git-attributes"],
        extensions: ["gitattributes"],
    },
    gleam => {
        feature: "lang-gleam",
        name: "gleam",
        ts_lang: arborium_gleam::language(),
        env_suffix: "GLEAM",
        names: ["gleam"],
        extensions: ["gleam"],
    },
    glsl => {
        feature: "lang-glsl",
        name: "glsl",
        ts_lang: arborium_glsl::language(),
        env_suffix: "GLSL",
        names: ["glsl"],
        extensions: ["glsl", "vert", "frag", "geom", "tesc", "tese", "comp"],
    },
    go => {
        feature: "lang-go",
        name: "go",
        ts_lang: arborium_go::language(),
        env_suffix: "GO",
        names: ["go", "golang"],
        extensions: ["go"],
    },
    graphql => {
        feature: "lang-graphql",
        name: "graphql",
        ts_lang: arborium_graphql::language(),
        env_suffix: "GRAPHQL",
        names: ["graphql", "gql"],
        extensions: ["graphql", "gql"],
    },
    groovy => {
        feature: "lang-groovy",
        name: "groovy",
        ts_lang: arborium_groovy::language(),
        env_suffix: "GROOVY",
        names: ["groovy", "gradle"],
        extensions: ["groovy", "gradle", "gvy", "gy", "gsh"],
    },
    haskell => {
        feature: "lang-haskell",
        name: "haskell",
        ts_lang: arborium_haskell::language(),
        env_suffix: "HASKELL",
        names: ["haskell", "hs"],
        extensions: ["hs", "lhs"],
    },
    hcl => {
        feature: "lang-hcl",
        name: "hcl",
        ts_lang: arborium_hcl::language(),
        env_suffix: "HCL",
        names: ["hcl", "terraform", "tf"],
        extensions: ["hcl", "tf", "tfvars"],
    },
    hlsl => {
        feature: "lang-hlsl",
        name: "hlsl",
        ts_lang: arborium_hlsl::language(),
        env_suffix: "HLSL",
        names: ["hlsl"],
        extensions: ["hlsl", "hlsli", "fx"],
    },
    html => {
        feature: "lang-html",
        name: "html",
        ts_lang: arborium_html::language(),
        env_suffix: "HTML",
        names: ["html", "htm"],
        extensions: ["html", "htm"],
    },
    idris => {
        feature: "lang-idris",
        name: "idris",
        ts_lang: arborium_idris::language(),
        env_suffix: "IDRIS",
        names: ["idris"],
        extensions: ["idr"],
    },
    ini => {
        feature: "lang-ini",
        name: "ini",
        ts_lang: arborium_ini::language(),
        env_suffix: "INI",
        names: ["ini"],
        extensions: ["ini", "cfg", "conf"],
    },
    java => {
        feature: "lang-java",
        name: "java",
        ts_lang: arborium_java::language(),
        env_suffix: "JAVA",
        names: ["java"],
        extensions: ["java"],
    },
    javascript => {
        feature: "lang-javascript",
        name: "javascript",
        ts_lang: arborium_javascript::language(),
        env_suffix: "JAVASCRIPT",
        names: ["javascript", "js", "jsx", "ecmascript", "es"],
        extensions: ["js", "mjs", "cjs", "jsx"],
    },
    jinja2 => {
        feature: "lang-jinja2",
        name: "jinja2",
        ts_lang: arborium_jinja2::language(),
        env_suffix: "JINJA2",
        names: ["jinja2", "jinja"],
        extensions: ["j2", "jinja", "jinja2"],
    },
    jq => {
        feature: "lang-jq",
        name: "jq",
        ts_lang: arborium_jq::language(),
        env_suffix: "JQ",
        names: ["jq"],
        extensions: ["jq"],
    },
    jsdoc => {
        feature: "lang-jsdoc",
        name: "jsdoc",
        ts_lang: arborium_jsdoc::language(),
        env_suffix: "JSDOC",
        names: ["jsdoc"],
        extensions: ["jsdoc"],
    },
    json => {
        feature: "lang-json",
        name: "json",
        ts_lang: arborium_json::language(),
        env_suffix: "JSON",
        names: ["json"],
        extensions: ["json"],
    },
    julia => {
        feature: "lang-julia",
        name: "julia",
        ts_lang: arborium_julia::language(),
        env_suffix: "JULIA",
        names: ["julia", "jl"],
        extensions: ["jl"],
    },
    just => {
        feature: "lang-just",
        name: "just",
        ts_lang: arborium_just::language(),
        env_suffix: "JUST",
        names: ["just", "justfile"],
        extensions: ["just"],
    },
    kconfig => {
        feature: "lang-kconfig",
        name: "kconfig",
        ts_lang: arborium_kconfig::language(),
        env_suffix: "KCONFIG",
        names: ["kconfig"],
        extensions: ["kconfig"],
    },
    kdl => {
        feature: "lang-kdl",
        name: "kdl",
        ts_lang: arborium_kdl::language(),
        env_suffix: "KDL",
        names: ["kdl"],
        extensions: ["kdl"],
    },
    kotlin => {
        feature: "lang-kotlin",
        name: "kotlin",
        ts_lang: arborium_kotlin::language(),
        env_suffix: "KOTLIN",
        names: ["kotlin", "kt"],
        extensions: ["kt", "kts"],
    },
    lean => {
        feature: "lang-lean",
        name: "lean",
        ts_lang: arborium_lean::language(),
        env_suffix: "LEAN",
        names: ["lean", "lean4"],
        extensions: ["lean"],
    },
    lua => {
        feature: "lang-lua",
        name: "lua",
        ts_lang: arborium_lua::language(),
        env_suffix: "LUA",
        names: ["lua"],
        extensions: ["lua"],
    },
    make => {
        feature: "lang-make",
        name: "make",
        ts_lang: arborium_make::language(),
        env_suffix: "MAKE",
        names: ["make", "makefile"],
        extensions: ["mk", "makefile"],
    },
    markdown => {
        feature: "lang-markdown",
        name: "markdown",
        ts_lang: arborium_markdown::language(),
        env_suffix: "MARKDOWN",
        names: ["markdown", "md"],
        extensions: ["md", "markdown"],
    },
    matlab => {
        feature: "lang-matlab",
        name: "matlab",
        ts_lang: arborium_matlab::language(),
        env_suffix: "MATLAB",
        names: ["matlab", "octave"],
        extensions: ["m"],
    },
    meson => {
        feature: "lang-meson",
        name: "meson",
        ts_lang: arborium_meson::language(),
        env_suffix: "MESON",
        names: ["meson"],
        extensions: ["meson"],
    },
    nginx => {
        feature: "lang-nginx",
        name: "nginx",
        ts_lang: arborium_nginx::language(),
        env_suffix: "NGINX",
        names: ["nginx"],
        extensions: ["nginx"],
    },
    ninja => {
        feature: "lang-ninja",
        name: "ninja",
        ts_lang: arborium_ninja::language(),
        env_suffix: "NINJA",
        names: ["ninja"],
        extensions: ["ninja"],
    },
    nix => {
        feature: "lang-nix",
        name: "nix",
        ts_lang: arborium_nix::language(),
        env_suffix: "NIX",
        names: ["nix"],
        extensions: ["nix"],
    },
    objc => {
        feature: "lang-objc",
        name: "objc",
        ts_lang: arborium_objc::language(),
        env_suffix: "OBJC",
        names: ["objc", "objective-c", "objectivec"],
        extensions: ["m", "mm"],
    },
    ocaml => {
        feature: "lang-ocaml",
        name: "ocaml",
        ts_lang: arborium_ocaml::language(),
        env_suffix: "OCAML",
        names: ["ocaml", "ml"],
        extensions: ["ml", "mli"],
    },
    odin => {
        feature: "lang-odin",
        name: "odin",
        ts_lang: arborium_odin::language(),
        env_suffix: "ODIN",
        names: ["odin"],
        extensions: ["odin"],
    },
    perl => {
        feature: "lang-perl",
        name: "perl",
        ts_lang: arborium_perl::language(),
        env_suffix: "PERL",
        names: ["perl", "pl"],
        extensions: ["pl", "pm"],
    },
    php => {
        feature: "lang-php",
        name: "php",
        ts_lang: arborium_php::language(),
        env_suffix: "PHP",
        names: ["php"],
        extensions: ["php"],
    },
    postscript => {
        feature: "lang-postscript",
        name: "postscript",
        ts_lang: arborium_postscript::language(),
        env_suffix: "POSTSCRIPT",
        names: ["postscript", "ps"],
        extensions: ["ps", "eps"],
    },
    powershell => {
        feature: "lang-powershell",
        name: "powershell",
        ts_lang: arborium_powershell::language(),
        env_suffix: "POWERSHELL",
        names: ["powershell", "pwsh", "ps1"],
        extensions: ["ps1", "psm1", "psd1"],
    },
    prolog => {
        feature: "lang-prolog",
        name: "prolog",
        ts_lang: arborium_prolog::language(),
        env_suffix: "PROLOG",
        names: ["prolog"],
        extensions: ["pl", "pro"],
    },
    proto => {
        feature: "lang-proto",
        name: "proto",
        ts_lang: arborium_proto::language(),
        env_suffix: "PROTO",
        names: ["proto", "protobuf", "protocol-buffers"],
        extensions: ["proto"],
    },
    python => {
        feature: "lang-python",
        name: "python",
        ts_lang: arborium_python::language(),
        env_suffix: "PYTHON",
        names: ["python", "py"],
        extensions: ["py", "pyi", "pyw"],
    },
    query => {
        feature: "lang-query",
        name: "query",
        ts_lang: arborium_query::language(),
        env_suffix: "QUERY",
        names: ["query", "scm"],
        extensions: ["scm"],
    },
    r => {
        feature: "lang-r",
        name: "r",
        ts_lang: arborium_r::language(),
        env_suffix: "R",
        names: ["r", "rlang"],
        extensions: ["r", "R"],
    },
    regex => {
        feature: "lang-regex",
        name: "regex",
        ts_lang: arborium_regex::language(),
        env_suffix: "REGEX",
        names: ["regex", "regexp", "regular-expression"],
        extensions: ["regex"],
    },
    rego => {
        feature: "lang-rego",
        name: "rego",
        ts_lang: arborium_rego::language(),
        env_suffix: "REGO",
        names: ["rego"],
        extensions: ["rego"],
    },
    rescript => {
        feature: "lang-rescript",
        name: "rescript",
        ts_lang: arborium_rescript::language(),
        env_suffix: "RESCRIPT",
        names: ["rescript", "res"],
        extensions: ["res", "resi"],
    },
    ron => {
        feature: "lang-ron",
        name: "ron",
        ts_lang: arborium_ron::language(),
        env_suffix: "RON",
        names: ["ron"],
        extensions: ["ron"],
    },
    ruby => {
        feature: "lang-ruby",
        name: "ruby",
        ts_lang: arborium_ruby::language(),
        env_suffix: "RUBY",
        names: ["ruby", "rb"],
        extensions: ["rb", "rake", "gemspec"],
    },
    rust => {
        feature: "lang-rust",
        name: "rust",
        ts_lang: arborium_rust::language(),
        env_suffix: "RUST",
        names: ["rust", "rs"],
        extensions: ["rs"],
    },
    scala => {
        feature: "lang-scala",
        name: "scala",
        ts_lang: arborium_scala::language(),
        env_suffix: "SCALA",
        names: ["scala"],
        extensions: ["scala", "sc"],
    },
    scheme => {
        feature: "lang-scheme",
        name: "scheme",
        ts_lang: arborium_scheme::language(),
        env_suffix: "SCHEME",
        names: ["scheme", "racket"],
        extensions: ["scm", "ss", "rkt"],
    },
    scss => {
        feature: "lang-scss",
        name: "scss",
        ts_lang: arborium_scss::language(),
        env_suffix: "SCSS",
        names: ["scss", "sass"],
        extensions: ["scss", "sass"],
    },
    solidity => {
        feature: "lang-solidity",
        name: "solidity",
        ts_lang: arborium_solidity::language(),
        env_suffix: "SOLIDITY",
        names: ["solidity", "sol"],
        extensions: ["sol"],
    },
    sparql => {
        feature: "lang-sparql",
        name: "sparql",
        ts_lang: arborium_sparql::language(),
        env_suffix: "SPARQL",
        names: ["sparql"],
        extensions: ["sparql", "rq"],
    },
    sql => {
        feature: "lang-sql",
        name: "sql",
        ts_lang: arborium_sql::language(),
        env_suffix: "SQL",
        names: ["sql"],
        extensions: ["sql"],
    },
    ssh_config => {
        feature: "lang-ssh-config",
        name: "ssh_config",
        ts_lang: arborium_ssh_config::language(),
        env_suffix: "SSH_CONFIG",
        names: ["ssh-config", "ssh_config", "sshconfig"],
        extensions: ["ssh_config"],
    },
    starlark => {
        feature: "lang-starlark",
        name: "starlark",
        ts_lang: arborium_starlark::language(),
        env_suffix: "STARLARK",
        names: ["starlark", "bazel", "bzl"],
        extensions: ["bzl", "bazel"],
    },
    styx => {
        feature: "lang-styx",
        name: "styx",
        ts_lang: arborium_styx::language(),
        env_suffix: "STYX",
        names: ["styx"],
        extensions: ["styx"],
    },
    svelte => {
        feature: "lang-svelte",
        name: "svelte",
        ts_lang: arborium_svelte::language(),
        env_suffix: "SVELTE",
        names: ["svelte"],
        extensions: ["svelte"],
    },
    swift => {
        feature: "lang-swift",
        name: "swift",
        ts_lang: arborium_swift::language(),
        env_suffix: "SWIFT",
        names: ["swift"],
        extensions: ["swift"],
    },
    textproto => {
        feature: "lang-textproto",
        name: "textproto",
        ts_lang: arborium_textproto::language(),
        env_suffix: "TEXTPROTO",
        names: ["textproto", "pbtxt"],
        extensions: ["textproto", "pbtxt"],
    },
    thrift => {
        feature: "lang-thrift",
        name: "thrift",
        ts_lang: arborium_thrift::language(),
        env_suffix: "THRIFT",
        names: ["thrift"],
        extensions: ["thrift"],
    },
    tlaplus => {
        feature: "lang-tlaplus",
        name: "tlaplus",
        ts_lang: arborium_tlaplus::language(),
        env_suffix: "TLAPLUS",
        names: ["tlaplus", "tla+", "tla"],
        extensions: ["tla"],
    },
    toml => {
        feature: "lang-toml",
        name: "toml",
        ts_lang: arborium_toml::language(),
        env_suffix: "TOML",
        names: ["toml"],
        extensions: ["toml"],
    },
    tsx => {
        feature: "lang-tsx",
        name: "tsx",
        ts_lang: arborium_tsx::language(),
        env_suffix: "TSX",
        names: ["tsx"],
        extensions: ["tsx"],
    },
    typescript => {
        feature: "lang-typescript",
        name: "typescript",
        ts_lang: arborium_typescript::language(),
        env_suffix: "TYPESCRIPT",
        names: ["typescript", "ts"],
        extensions: ["ts", "mts", "cts"],
    },
    typst => {
        feature: "lang-typst",
        name: "typst",
        ts_lang: arborium_typst::language(),
        env_suffix: "TYPST",
        names: ["typst"],
        extensions: ["typ"],
    },
    uiua => {
        feature: "lang-uiua",
        name: "uiua",
        ts_lang: arborium_uiua::language(),
        env_suffix: "UIUA",
        names: ["uiua"],
        extensions: ["ua"],
    },
    vb => {
        feature: "lang-vb",
        name: "vb",
        ts_lang: arborium_vb::language(),
        env_suffix: "VB",
        names: ["vb", "vbnet", "visualbasic"],
        extensions: ["vb"],
    },
    verilog => {
        feature: "lang-verilog",
        name: "verilog",
        ts_lang: arborium_verilog::language(),
        env_suffix: "VERILOG",
        names: ["verilog", "v"],
        extensions: ["v", "sv"],
    },
    vhdl => {
        feature: "lang-vhdl",
        name: "vhdl",
        ts_lang: arborium_vhdl::language(),
        env_suffix: "VHDL",
        names: ["vhdl"],
        extensions: ["vhd", "vhdl"],
    },
    vim => {
        feature: "lang-vim",
        name: "vim",
        ts_lang: arborium_vim::language(),
        env_suffix: "VIM",
        names: ["vim", "vimscript"],
        extensions: ["vim"],
    },
    vue => {
        feature: "lang-vue",
        name: "vue",
        ts_lang: arborium_vue::language(),
        env_suffix: "VUE",
        names: ["vue"],
        extensions: ["vue"],
    },
    wit => {
        feature: "lang-wit",
        name: "wit",
        ts_lang: arborium_wit::language(),
        env_suffix: "WIT",
        names: ["wit"],
        extensions: ["wit"],
    },
    x86asm => {
        feature: "lang-x86asm",
        name: "x86asm",
        ts_lang: arborium_x86asm::language(),
        env_suffix: "X86ASM",
        names: ["x86asm", "x86"],
        extensions: ["asm"],
    },
    xml => {
        feature: "lang-xml",
        name: "xml",
        ts_lang: arborium_xml::language(),
        env_suffix: "XML",
        names: ["xml"],
        extensions: ["xml", "xsl", "xslt", "xsd", "svg"],
    },
    yaml => {
        feature: "lang-yaml",
        name: "yaml",
        ts_lang: arborium_yaml::language(),
        env_suffix: "YAML",
        names: ["yaml", "yml"],
        extensions: ["yaml", "yml"],
    },
    yuri => {
        feature: "lang-yuri",
        name: "yuri",
        ts_lang: arborium_yuri::language(),
        env_suffix: "YURI",
        names: ["yuri"],
        extensions: ["yuri"],
    },
    zig => {
        feature: "lang-zig",
        name: "zig",
        ts_lang: arborium_zig::language(),
        env_suffix: "ZIG",
        names: ["zig"],
        extensions: ["zig"],
    },
    zsh => {
        feature: "lang-zsh",
        name: "zsh",
        ts_lang: arborium_zsh::language(),
        env_suffix: "ZSH",
        names: ["zsh"],
        extensions: ["zsh"],
    },
}
