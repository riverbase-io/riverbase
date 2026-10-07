use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote_spanned;
use syn::{parse::Parse, punctuated::Punctuated, Expr, FnArg, ItemFn, LitStr, Meta, Pat, Token};

struct DomainActionAttr {
    event: LitStr,
    resources: Vec<LitStr>,
}

impl Parse for DomainActionAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut event = None;
        let mut resources = Vec::new();

        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for meta in metas {
            let Meta::NameValue(nv) = meta else {
                return Err(syn::Error::new_spanned(
                    meta,
                    "expected `event = \"...\"` style attributes",
                ));
            };
            if nv.path.is_ident("event") {
                event = Some(parse_lit_str(&nv.value)?);
            } else if nv.path.is_ident("resources") {
                resources = parse_lit_str_array(&nv.value)?;
            } else {
                return Err(syn::Error::new_spanned(
                    nv.path,
                    "unknown key; supported: event, resources",
                ));
            }
        }

        let event = event.ok_or_else(|| {
            syn::Error::new(
                input.span(),
                "`event = \"...\"` is required for #[domain_action]",
            )
        })?;

        Ok(Self { event, resources })
    }
}

fn parse_lit_str(expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Ok(s.clone()),
        other => Err(syn::Error::new_spanned(other, "expected a string literal")),
    }
}

fn parse_lit_str_array(expr: &Expr) -> syn::Result<Vec<LitStr>> {
    match expr {
        Expr::Array(array) => array.elems.iter().map(parse_lit_str).collect(),
        other => Err(syn::Error::new_spanned(
            other,
            "expected an array of string literals, e.g. `[\"goal\"]`",
        )),
    }
}

pub fn domain_action(attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let attr = syn::parse2::<DomainActionAttr>(attr)?;
    let mut input_fn = syn::parse2::<ItemFn>(item)?;

    if input_fn.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            input_fn.sig.fn_token,
            "#[domain_action] requires an async fn",
        ));
    }

    input_fn
        .attrs
        .retain(|a| !a.path().is_ident("domain_action"));

    let event_type = attr.event.value();
    let resources = attr.resources.iter().map(LitStr::value).collect::<Vec<_>>();
    let span = Span::call_site();

    let input_inserts = build_input_inserts(&input_fn.sig)?;
    let original_block = input_fn.block.clone();
    let resource_lits = resources.iter().map(|resource| {
        quote_spanned! { span => #resource }
    });

    let wrapped_block = quote_spanned! { span =>
        {
            let __flrs_input = {
                let mut __flrs_input_map = serde_json::Map::new();
                #(#input_inserts)*
                serde_json::Value::Object(__flrs_input_map)
            };

            self.core.begin_domain_action(&[#(#resource_lits),*])?;

            let __flrs_result = async {
                #original_block
            }
            .await;

            let __flrs_final = if let Ok(__flrs_val) = &__flrs_result {
                let __flrs_event_payload = serde_json::to_value(__flrs_val)
                    .unwrap_or(serde_json::Value::Null);
                match self
                    .core
                    .enqueue_action_event(#event_type, __flrs_input, __flrs_event_payload)
                    .await
                {
                    Ok(()) => __flrs_result,
                    Err(__flrs_err) => Err(__flrs_err),
                }
            } else {
                __flrs_result
            };

            self.core.end_domain_action();
            __flrs_final
        }
    };

    input_fn.block = syn::parse2(wrapped_block)?;

    Ok(quote_spanned! { span => #input_fn })
}

fn build_input_inserts(sig: &syn::Signature) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let mut inserts = Vec::new();
    let span = Span::call_site();

    for arg in &sig.inputs {
        let FnArg::Typed(pat_type) = arg else {
            continue;
        };

        let pat = match &*pat_type.pat {
            Pat::Ident(pat_ident) => pat_ident,
            Pat::Wild(_) => {
                return Err(syn::Error::new_spanned(
                    pat_type,
                    "#[domain_action] requires named parameters (not `_`)",
                ));
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "#[domain_action] supports only simple identifier parameters",
                ));
            }
        };

        if pat.ident == "self" {
            continue;
        }

        let name = pat.ident.clone();
        let name_str = name.to_string();
        inserts.push(quote_spanned! { span =>
            __flrs_input_map.insert(
                #name_str.to_string(),
                serde_json::to_value(&#name).unwrap_or(serde_json::Value::Null),
            );
        });
    }

    Ok(inserts)
}
