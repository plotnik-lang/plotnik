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
        #[cfg(feature = "bash")]
        "bash" => tree_sitter_bash::LANGUAGE.into(),
        #[cfg(feature = "c")]
        "c" => tree_sitter_c::LANGUAGE.into(),
        #[cfg(feature = "cpp")]
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        #[cfg(feature = "csharp")]
        "csharp" => tree_sitter_c_sharp::LANGUAGE.into(),
        #[cfg(feature = "css")]
        "css" => tree_sitter_css::LANGUAGE.into(),
        #[cfg(feature = "elixir")]
        "elixir" => tree_sitter_elixir::LANGUAGE.into(),
        #[cfg(feature = "go")]
        "go" => tree_sitter_go::LANGUAGE.into(),
        #[cfg(feature = "haskell")]
        "haskell" => tree_sitter_haskell::LANGUAGE.into(),
        #[cfg(feature = "hcl")]
        "hcl" => tree_sitter_hcl::LANGUAGE.into(),
        #[cfg(feature = "html")]
        "html" => tree_sitter_html::LANGUAGE.into(),
        #[cfg(feature = "java")]
        "java" => tree_sitter_java::LANGUAGE.into(),
        #[cfg(feature = "javascript")]
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        #[cfg(feature = "json")]
        "json" => tree_sitter_json::LANGUAGE.into(),
        #[cfg(feature = "kotlin")]
        "kotlin" => tree_sitter_kotlin::LANGUAGE.into(),
        #[cfg(feature = "lua")]
        "lua" => tree_sitter_lua::LANGUAGE.into(),
        #[cfg(feature = "nix")]
        "nix" => tree_sitter_nix::LANGUAGE.into(),
        #[cfg(feature = "php")]
        "php" => tree_sitter_php::LANGUAGE_PHP.into(),
        #[cfg(feature = "python")]
        "python" => tree_sitter_python::LANGUAGE.into(),
        #[cfg(feature = "ruby")]
        "ruby" => tree_sitter_ruby::LANGUAGE.into(),
        #[cfg(feature = "rust")]
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        #[cfg(feature = "scala")]
        "scala" => tree_sitter_scala::LANGUAGE.into(),
        #[cfg(feature = "solidity")]
        "solidity" => tree_sitter_solidity::LANGUAGE.into(),
        #[cfg(feature = "swift")]
        "swift" => tree_sitter_swift::LANGUAGE.into(),
        #[cfg(feature = "typescript")]
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        #[cfg(feature = "typescript")]
        "typescript_tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        #[cfg(feature = "yaml")]
        "yaml" => tree_sitter_yaml::LANGUAGE.into(),
        _ => panic!("Unknown or disabled language key: {}", key),
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

    // Process nodes in sorted order (for binary search on node lookup)
    let sorted_node_ids = node_types.sorted_node_ids();

    for node_id in &sorted_node_ids {
        let info = node_types.get(*node_id).unwrap();

        let mut field_array_defs = Vec::new();
        let mut field_entries = Vec::new();

        // Sort fields by field_id (for binary search on field lookup)
        let mut sorted_fields: Vec<_> = info.fields.iter().collect();
        sorted_fields.sort_by_key(|(fid, _)| *fid);

        for (field_id, field_info) in &sorted_fields {
            let valid_types = field_info.valid_types.to_vec();

            let valid_types_name = syn::Ident::new(
                &format!("{}_N{}_F{}_TYPES", prefix, node_id, field_id),
                Span::call_site(),
            );

            let multiple = field_info.cardinality.multiple;
            let required = field_info.cardinality.required;
            let types_len = valid_types.len();

            field_array_defs.push(quote! {
                static #valid_types_name: [u16; #types_len] = [#(#valid_types),*];
            });

            let field_id_raw = field_id.get();
            field_entries.push(quote! {
                (std::num::NonZeroU16::new(#field_id_raw).unwrap(), plotnik_core::StaticFieldInfo {
                    cardinality: plotnik_core::Cardinality {
                        multiple: #multiple,
                        required: #required,
                    },
                    valid_types: &#valid_types_name,
                })
            });
        }

        let fields_array_name = syn::Ident::new(
            &format!("{}_N{}_FIELDS", prefix, node_id),
            Span::call_site(),
        );
        let fields_len = sorted_fields.len();

        static_defs.extend(field_array_defs);

        if !sorted_fields.is_empty() {
            static_defs.push(quote! {
                static #fields_array_name: [(std::num::NonZeroU16, plotnik_core::StaticFieldInfo); #fields_len] = [
                    #(#field_entries),*
                ];
            });
        }

        let children_code = if let Some(children) = &info.children {
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
        } else {
            quote! { None }
        };

        let name = &info.name;
        let named = info.named;

        let fields_ref = if sorted_fields.is_empty() {
            quote! { &[] }
        } else {
            quote! { &#fields_array_name }
        };

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
