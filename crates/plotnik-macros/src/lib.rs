use proc_macro::TokenStream;
use quote::quote;
use syn::{LitStr, parse_macro_input};

#[proc_macro]
pub fn generate_node_types_size(input: TokenStream) -> TokenStream {
    let lang = parse_macro_input!(input as LitStr).value();
    let env_var = format!("PLOTNIK_NODE_TYPES_{}", lang.to_uppercase());

    let path = std::env::var(&env_var).unwrap_or_else(|_| {
        panic!(
            "Environment variable {} not set. Is build.rs configured correctly?",
            env_var
        )
    });

    let size = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
        .len();

    let const_name = syn::Ident::new(
        &format!("{}_NODE_TYPES_SIZE", lang.to_uppercase()),
        proc_macro2::Span::call_site(),
    );

    quote! {
        pub const #const_name: usize = #size;
    }
    .into()
}
