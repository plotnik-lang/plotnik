use arborium_tree_sitter::Language;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{LitStr, parse_macro_input};

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
        #[cfg(feature = "lang-bash")]
        "bash" => arborium_bash::language().into(),
        #[cfg(feature = "lang-c")]
        "c" => arborium_c::language().into(),
        #[cfg(feature = "lang-cpp")]
        "cpp" => arborium_cpp::language().into(),
        #[cfg(feature = "lang-c-sharp")]
        "c_sharp" => arborium_c_sharp::language().into(),
        #[cfg(feature = "lang-css")]
        "css" => arborium_css::language().into(),
        #[cfg(feature = "lang-elixir")]
        "elixir" => arborium_elixir::language().into(),
        #[cfg(feature = "lang-go")]
        "go" => arborium_go::language().into(),
        #[cfg(feature = "lang-haskell")]
        "haskell" => arborium_haskell::language().into(),
        #[cfg(feature = "lang-hcl")]
        "hcl" => arborium_hcl::language().into(),
        #[cfg(feature = "lang-html")]
        "html" => arborium_html::language().into(),
        #[cfg(feature = "lang-java")]
        "java" => arborium_java::language().into(),
        #[cfg(feature = "lang-javascript")]
        "javascript" => arborium_javascript::language().into(),
        #[cfg(feature = "lang-json")]
        "json" => arborium_json::language().into(),
        #[cfg(feature = "lang-kotlin")]
        "kotlin" => arborium_kotlin::language().into(),
        #[cfg(feature = "lang-lua")]
        "lua" => arborium_lua::language().into(),
        #[cfg(feature = "lang-nix")]
        "nix" => arborium_nix::language().into(),
        #[cfg(feature = "lang-php")]
        "php" => arborium_php::language().into(),
        #[cfg(feature = "lang-python")]
        "python" => arborium_python::language().into(),
        #[cfg(feature = "lang-ruby")]
        "ruby" => arborium_ruby::language().into(),
        #[cfg(feature = "lang-rust")]
        "rust" => arborium_rust::language().into(),
        #[cfg(feature = "lang-scala")]
        "scala" => arborium_scala::language().into(),
        #[cfg(feature = "lang-swift")]
        "swift" => arborium_swift::language().into(),
        #[cfg(feature = "lang-typescript")]
        "typescript" => arborium_typescript::language().into(),
        #[cfg(feature = "lang-tsx")]
        "tsx" => arborium_tsx::language().into(),
        #[cfg(feature = "lang-yaml")]
        "yaml" => arborium_yaml::language().into(),
        _ => panic!("Unknown or disabled language key: {}", key),
    }
}

struct FieldCodeGen {
    array_defs: Vec<proc_macro2::TokenStream>,
    entries: Vec<proc_macro2::TokenStream>,
}

fn generate_field_code(
    prefix: &str,
    node_id: u16,
    field_id: &std::num::NonZeroU16,
    field_info: &plotnik_core::FieldInfo,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let valid_types = field_info.valid_types.to_vec();
    let valid_types_name = syn::Ident::new(
        &format!("{}_N{}_F{}_TYPES", prefix, node_id, field_id),
        Span::call_site(),
    );

    let multiple = field_info.cardinality.multiple;
    let required = field_info.cardinality.required;
    let types_len = valid_types.len();

    let array_def = quote! {
        static #valid_types_name: [u16; #types_len] = [#(#valid_types),*];
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
    node_id: u16,
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
    node_id: u16,
    children: &plotnik_core::ChildrenInfo,
    static_defs: &mut Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let valid_types = children.valid_types.to_vec();
    let children_types_name = syn::Ident::new(
        &format!("{}_N{}_CHILDREN_TYPES", prefix, node_id),
        Span::call_site(),
    );
    let types_len = valid_types.len();

    static_defs.push(quote! {
        static #children_types_name: [u16; #types_len] = [#(#valid_types),*];
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
            if id == 0 && named { None } else { Some(id) }
        },
        |name| ts_lang.field_id_for_name(name),
    );

    let prefix = lang_key.to_uppercase();
    let mut static_defs = Vec::new();
    let mut node_entries = Vec::new();

    let extras = node_types.sorted_extras();
    let root = node_types.root();
    let sorted_node_ids = node_types.sorted_node_ids();

    for &node_id in &sorted_node_ids {
        let info = node_types.get(node_id).unwrap();

        let field_gen = generate_fields_for_node(&prefix, node_id, &info.fields);
        static_defs.extend(field_gen.array_defs);

        let fields_ref = if field_gen.entries.is_empty() {
            quote! { &[] }
        } else {
            let fields_array_name = syn::Ident::new(
                &format!("{}_N{}_FIELDS", prefix, node_id),
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
            (#node_id, plotnik_core::StaticNodeTypeInfo {
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
    let extras_len = extras.len();

    let root_code = match root {
        Some(id) => quote! { Some(#id) },
        None => quote! { None },
    };

    quote! {
        #(#static_defs)*

        static #nodes_array_name: [(u16, plotnik_core::StaticNodeTypeInfo); #nodes_len] = [
            #(#node_entries),*
        ];

        static #extras_array_name: [u16; #extras_len] = [#(#extras),*];

        pub static #const_name: plotnik_core::StaticNodeTypes = plotnik_core::StaticNodeTypes::new(
            &#nodes_array_name,
            &#extras_array_name,
            #root_code,
        );
    }
}
