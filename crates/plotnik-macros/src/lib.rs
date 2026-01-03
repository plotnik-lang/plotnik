use arborium_tree_sitter as tree_sitter;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{LitStr, parse_macro_input};
use tree_sitter::Language;

use plotnik_core::NodeTypes;

/// Generate a StaticNodeTypes constant for a language.
///
/// Usage: `generate_node_types!("javascript")`
///
/// This reads the node-types.json at compile time and uses the tree-sitter
/// Language to resolve node/field names to IDs, producing efficient lookup tables.
/// The output is fully statically allocated - no runtime initialization needed.
#[proc_macro]
pub fn generate_node_types(input: TokenStream) -> TokenStream {
    let lang_key = parse_macro_input!(input as LitStr).value();

    let env_var = format!("PLOTNIK_NODE_TYPES_{}", lang_key.to_uppercase());

    let json_path = std::env::var(&env_var).unwrap_or_else(|_| {
        panic!(
            "Environment variable {} not set. Is build.rs configured correctly?",
            env_var
        )
    });

    let json_content = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", json_path, e));

    let raw_nodes: Vec<plotnik_core::RawNode> = serde_json::from_str(&json_content)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", json_path, e));

    let ts_lang = get_language_for_key(&lang_key);

    let const_name = syn::Ident::new(
        &format!("{}_NODE_TYPES", lang_key.to_uppercase()),
        Span::call_site(),
    );

    let generated = generate_static_node_types_code(&raw_nodes, &ts_lang, &lang_key, &const_name);

    generated.into()
}

fn get_language_for_key(key: &str) -> Language {
    match key.to_lowercase().as_str() {
        #[cfg(feature = "lang-ada")]
        "ada" => arborium_ada::language().into(),
        #[cfg(feature = "lang-agda")]
        "agda" => arborium_agda::language().into(),
        #[cfg(feature = "lang-asciidoc")]
        "asciidoc" => arborium_asciidoc::language().into(),
        #[cfg(feature = "lang-asm")]
        "asm" => arborium_asm::language().into(),
        #[cfg(feature = "lang-awk")]
        "awk" => arborium_awk::language().into(),
        #[cfg(feature = "lang-bash")]
        "bash" => arborium_bash::language().into(),
        #[cfg(feature = "lang-batch")]
        "batch" => arborium_batch::language().into(),
        #[cfg(feature = "lang-c")]
        "c" => arborium_c::language().into(),
        #[cfg(feature = "lang-c-sharp")]
        "c_sharp" => arborium_c_sharp::language().into(),
        #[cfg(feature = "lang-caddy")]
        "caddy" => arborium_caddy::language().into(),
        #[cfg(feature = "lang-capnp")]
        "capnp" => arborium_capnp::language().into(),
        #[cfg(feature = "lang-clojure")]
        "clojure" => arborium_clojure::language().into(),
        #[cfg(feature = "lang-cmake")]
        "cmake" => arborium_cmake::language().into(),
        #[cfg(feature = "lang-commonlisp")]
        "commonlisp" => arborium_commonlisp::language().into(),
        #[cfg(feature = "lang-cpp")]
        "cpp" => arborium_cpp::language().into(),
        #[cfg(feature = "lang-css")]
        "css" => arborium_css::language().into(),
        #[cfg(feature = "lang-d")]
        "d" => arborium_d::language().into(),
        #[cfg(feature = "lang-dart")]
        "dart" => arborium_dart::language().into(),
        #[cfg(feature = "lang-devicetree")]
        "devicetree" => arborium_devicetree::language().into(),
        #[cfg(feature = "lang-diff")]
        "diff" => arborium_diff::language().into(),
        #[cfg(feature = "lang-dockerfile")]
        "dockerfile" => arborium_dockerfile::language().into(),
        #[cfg(feature = "lang-dot")]
        "dot" => arborium_dot::language().into(),
        #[cfg(feature = "lang-elisp")]
        "elisp" => arborium_elisp::language().into(),
        #[cfg(feature = "lang-elixir")]
        "elixir" => arborium_elixir::language().into(),
        #[cfg(feature = "lang-elm")]
        "elm" => arborium_elm::language().into(),
        #[cfg(feature = "lang-erlang")]
        "erlang" => arborium_erlang::language().into(),
        #[cfg(feature = "lang-fish")]
        "fish" => arborium_fish::language().into(),
        #[cfg(feature = "lang-fsharp")]
        "fsharp" => arborium_fsharp::language().into(),
        #[cfg(feature = "lang-gleam")]
        "gleam" => arborium_gleam::language().into(),
        #[cfg(feature = "lang-glsl")]
        "glsl" => arborium_glsl::language().into(),
        #[cfg(feature = "lang-go")]
        "go" => arborium_go::language().into(),
        #[cfg(feature = "lang-graphql")]
        "graphql" => arborium_graphql::language().into(),
        #[cfg(feature = "lang-groovy")]
        "groovy" => arborium_groovy::language().into(),
        #[cfg(feature = "lang-haskell")]
        "haskell" => arborium_haskell::language().into(),
        #[cfg(feature = "lang-hcl")]
        "hcl" => arborium_hcl::language().into(),
        #[cfg(feature = "lang-hlsl")]
        "hlsl" => arborium_hlsl::language().into(),
        #[cfg(feature = "lang-html")]
        "html" => arborium_html::language().into(),
        #[cfg(feature = "lang-idris")]
        "idris" => arborium_idris::language().into(),
        #[cfg(feature = "lang-ini")]
        "ini" => arborium_ini::language().into(),
        #[cfg(feature = "lang-java")]
        "java" => arborium_java::language().into(),
        #[cfg(feature = "lang-javascript")]
        "javascript" => arborium_javascript::language().into(),
        #[cfg(feature = "lang-jinja2")]
        "jinja2" => arborium_jinja2::language().into(),
        #[cfg(feature = "lang-jq")]
        "jq" => arborium_jq::language().into(),
        #[cfg(feature = "lang-json")]
        "json" => arborium_json::language().into(),
        #[cfg(feature = "lang-julia")]
        "julia" => arborium_julia::language().into(),
        #[cfg(feature = "lang-kdl")]
        "kdl" => arborium_kdl::language().into(),
        #[cfg(feature = "lang-kotlin")]
        "kotlin" => arborium_kotlin::language().into(),
        #[cfg(feature = "lang-lean")]
        "lean" => arborium_lean::language().into(),
        #[cfg(feature = "lang-lua")]
        "lua" => arborium_lua::language().into(),
        #[cfg(feature = "lang-markdown")]
        "markdown" => arborium_markdown::language().into(),
        #[cfg(feature = "lang-matlab")]
        "matlab" => arborium_matlab::language().into(),
        #[cfg(feature = "lang-meson")]
        "meson" => arborium_meson::language().into(),
        #[cfg(feature = "lang-nginx")]
        "nginx" => arborium_nginx::language().into(),
        #[cfg(feature = "lang-ninja")]
        "ninja" => arborium_ninja::language().into(),
        #[cfg(feature = "lang-nix")]
        "nix" => arborium_nix::language().into(),
        #[cfg(feature = "lang-objc")]
        "objc" => arborium_objc::language().into(),
        #[cfg(feature = "lang-ocaml")]
        "ocaml" => arborium_ocaml::language().into(),
        #[cfg(feature = "lang-perl")]
        "perl" => arborium_perl::language().into(),
        #[cfg(feature = "lang-php")]
        "php" => arborium_php::language().into(),
        #[cfg(feature = "lang-postscript")]
        "postscript" => arborium_postscript::language().into(),
        #[cfg(feature = "lang-powershell")]
        "powershell" => arborium_powershell::language().into(),
        #[cfg(feature = "lang-prolog")]
        "prolog" => arborium_prolog::language().into(),
        #[cfg(feature = "lang-python")]
        "python" => arborium_python::language().into(),
        #[cfg(feature = "lang-query")]
        "query" => arborium_query::language().into(),
        #[cfg(feature = "lang-r")]
        "r" => arborium_r::language().into(),
        #[cfg(feature = "lang-rescript")]
        "rescript" => arborium_rescript::language().into(),
        #[cfg(feature = "lang-ron")]
        "ron" => arborium_ron::language().into(),
        #[cfg(feature = "lang-ruby")]
        "ruby" => arborium_ruby::language().into(),
        #[cfg(feature = "lang-rust")]
        "rust" => arborium_rust::language().into(),
        #[cfg(feature = "lang-scala")]
        "scala" => arborium_scala::language().into(),
        #[cfg(feature = "lang-scheme")]
        "scheme" => arborium_scheme::language().into(),
        #[cfg(feature = "lang-scss")]
        "scss" => arborium_scss::language().into(),
        #[cfg(feature = "lang-sparql")]
        "sparql" => arborium_sparql::language().into(),
        #[cfg(feature = "lang-sql")]
        "sql" => arborium_sql::language().into(),
        #[cfg(feature = "lang-ssh-config")]
        "ssh_config" => arborium_ssh_config::language().into(),
        #[cfg(feature = "lang-starlark")]
        "starlark" => arborium_starlark::language().into(),
        #[cfg(feature = "lang-svelte")]
        "svelte" => arborium_svelte::language().into(),
        #[cfg(feature = "lang-swift")]
        "swift" => arborium_swift::language().into(),
        #[cfg(feature = "lang-textproto")]
        "textproto" => arborium_textproto::language().into(),
        #[cfg(feature = "lang-thrift")]
        "thrift" => arborium_thrift::language().into(),
        #[cfg(feature = "lang-tlaplus")]
        "tlaplus" => arborium_tlaplus::language().into(),
        #[cfg(feature = "lang-toml")]
        "toml" => arborium_toml::language().into(),
        #[cfg(feature = "lang-tsx")]
        "tsx" => arborium_tsx::language().into(),
        #[cfg(feature = "lang-typescript")]
        "typescript" => arborium_typescript::language().into(),
        #[cfg(feature = "lang-typst")]
        "typst" => arborium_typst::language().into(),
        #[cfg(feature = "lang-uiua")]
        "uiua" => arborium_uiua::language().into(),
        #[cfg(feature = "lang-vb")]
        "vb" => arborium_vb::language().into(),
        #[cfg(feature = "lang-verilog")]
        "verilog" => arborium_verilog::language().into(),
        #[cfg(feature = "lang-vhdl")]
        "vhdl" => arborium_vhdl::language().into(),
        #[cfg(feature = "lang-vim")]
        "vim" => arborium_vim::language().into(),
        #[cfg(feature = "lang-vue")]
        "vue" => arborium_vue::language().into(),
        #[cfg(feature = "lang-wit")]
        "wit" => arborium_wit::language().into(),
        #[cfg(feature = "lang-x86asm")]
        "x86asm" => arborium_x86asm::language().into(),
        #[cfg(feature = "lang-xml")]
        "xml" => arborium_xml::language().into(),
        #[cfg(feature = "lang-yaml")]
        "yaml" => arborium_yaml::language().into(),
        #[cfg(feature = "lang-yuri")]
        "yuri" => arborium_yuri::language().into(),
        #[cfg(feature = "lang-zig")]
        "zig" => arborium_zig::language().into(),
        #[cfg(feature = "lang-zsh")]
        "zsh" => arborium_zsh::language().into(),
        _ => panic!("Unknown or disabled language key: {}", key),
    }
}

struct FieldCodeGen {
    array_defs: Vec<proc_macro2::TokenStream>,
    entries: Vec<proc_macro2::TokenStream>,
}

fn generate_field_code(
    prefix: &str,
    node_id: std::num::NonZeroU16,
    field_id: &std::num::NonZeroU16,
    field_info: &plotnik_core::FieldInfo,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let valid_types_raw: Vec<u16> = field_info.valid_types.iter().map(|id| id.get()).collect();
    let valid_types_name = syn::Ident::new(
        &format!("{}_N{}_F{}_TYPES", prefix, node_id.get(), field_id),
        Span::call_site(),
    );

    let multiple = field_info.cardinality.multiple;
    let required = field_info.cardinality.required;
    let types_len = valid_types_raw.len();

    let array_def = quote! {
        static #valid_types_name: [std::num::NonZeroU16; #types_len] = [
            #(std::num::NonZeroU16::new(#valid_types_raw).unwrap()),*
        ];
    };

    let field_id_raw = field_id.get();
    let entry = quote! {
        (std::num::NonZeroU16::new(#field_id_raw).unwrap(), plotnik_core::StaticFieldInfo {
            cardinality: plotnik_core::Cardinality {
                multiple: #multiple,
                required: #required,
            },
            valid_types: &#valid_types_name,
        })
    };

    (array_def, entry)
}

fn generate_fields_for_node(
    prefix: &str,
    node_id: std::num::NonZeroU16,
    fields: &std::collections::HashMap<std::num::NonZeroU16, plotnik_core::FieldInfo>,
) -> FieldCodeGen {
    let mut sorted_fields: Vec<_> = fields.iter().collect();
    sorted_fields.sort_by_key(|(fid, _)| *fid);

    let mut array_defs = Vec::new();
    let mut entries = Vec::new();

    for (field_id, field_info) in sorted_fields {
        let (array_def, entry) = generate_field_code(prefix, node_id, field_id, field_info);
        array_defs.push(array_def);
        entries.push(entry);
    }

    FieldCodeGen {
        array_defs,
        entries,
    }
}

fn generate_children_code(
    prefix: &str,
    node_id: std::num::NonZeroU16,
    children: &plotnik_core::ChildrenInfo,
    static_defs: &mut Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let valid_types_raw: Vec<u16> = children.valid_types.iter().map(|id| id.get()).collect();
    let children_types_name = syn::Ident::new(
        &format!("{}_N{}_CHILDREN_TYPES", prefix, node_id.get()),
        Span::call_site(),
    );
    let types_len = valid_types_raw.len();

    static_defs.push(quote! {
        static #children_types_name: [std::num::NonZeroU16; #types_len] = [
            #(std::num::NonZeroU16::new(#valid_types_raw).unwrap()),*
        ];
    });

    let multiple = children.cardinality.multiple;
    let required = children.cardinality.required;

    quote! {
        Some(plotnik_core::StaticChildrenInfo {
            cardinality: plotnik_core::Cardinality {
                multiple: #multiple,
                required: #required,
            },
            valid_types: &#children_types_name,
        })
    }
}

fn generate_static_node_types_code(
    raw_nodes: &[plotnik_core::RawNode],
    ts_lang: &Language,
    lang_key: &str,
    const_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    let node_types = plotnik_core::DynamicNodeTypes::build(
        raw_nodes,
        |name, named| {
            let id = ts_lang.id_for_node_kind(name, named);
            std::num::NonZeroU16::new(id)
        },
        |name| ts_lang.field_id_for_name(name),
    );

    let prefix = lang_key.to_uppercase();
    let mut static_defs = Vec::new();
    let mut node_entries = Vec::new();

    let extras_raw: Vec<u16> = node_types
        .sorted_extras()
        .iter()
        .map(|id| id.get())
        .collect();
    let root = node_types.root();
    let sorted_node_ids = node_types.sorted_node_ids();

    for &node_id in &sorted_node_ids {
        let info = node_types.get(node_id).unwrap();

        let node_id_raw = node_id.get();
        let field_gen = generate_fields_for_node(&prefix, node_id, &info.fields);
        static_defs.extend(field_gen.array_defs);

        let fields_ref = if field_gen.entries.is_empty() {
            quote! { &[] }
        } else {
            let fields_array_name = syn::Ident::new(
                &format!("{}_N{}_FIELDS", prefix, node_id_raw),
                Span::call_site(),
            );
            let fields_len = field_gen.entries.len();
            let field_entries = &field_gen.entries;

            static_defs.push(quote! {
                static #fields_array_name: [(std::num::NonZeroU16, plotnik_core::StaticFieldInfo); #fields_len] = [
                    #(#field_entries),*
                ];
            });

            quote! { &#fields_array_name }
        };

        let children_code = match &info.children {
            Some(children) => generate_children_code(&prefix, node_id, children, &mut static_defs),
            None => quote! { None },
        };

        let name = &info.name;
        let named = info.named;

        node_entries.push(quote! {
            (std::num::NonZeroU16::new(#node_id_raw).unwrap(), plotnik_core::StaticNodeTypeInfo {
                name: #name,
                named: #named,
                fields: #fields_ref,
                children: #children_code,
            })
        });
    }

    let nodes_array_name = syn::Ident::new(&format!("{}_NODES", prefix), Span::call_site());
    let nodes_len = sorted_node_ids.len();

    let extras_array_name = syn::Ident::new(&format!("{}_EXTRAS", prefix), Span::call_site());
    let extras_len = extras_raw.len();

    let root_code = match root {
        Some(id) => {
            let id_raw = id.get();
            quote! { Some(std::num::NonZeroU16::new(#id_raw).unwrap()) }
        }
        None => quote! { None },
    };

    quote! {
        #(#static_defs)*

        static #nodes_array_name: [(std::num::NonZeroU16, plotnik_core::StaticNodeTypeInfo); #nodes_len] = [
            #(#node_entries),*
        ];

        static #extras_array_name: [std::num::NonZeroU16; #extras_len] = [
            #(std::num::NonZeroU16::new(#extras_raw).unwrap()),*
        ];

        pub static #const_name: plotnik_core::StaticNodeTypes = plotnik_core::StaticNodeTypes::new(
            &#nodes_array_name,
            &#extras_array_name,
            #root_code,
        );
    }
}
