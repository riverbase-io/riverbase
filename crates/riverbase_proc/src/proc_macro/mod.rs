//! Proc-macro implementations (`#[proc_macro_*]` entry points live in the crate root).

mod domain_action;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, LitStr};

pub fn domain_action(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain_action::domain_action(attr, item)
}

pub fn riverbase_namespace(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let value = lit.value();
    let expanded = quote! {
        riverbase_core::base::Namespace::new(#value)
    };
    expanded.into()
}
