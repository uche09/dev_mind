use syn::{Attribute, Expr, ExprLit, Lit};

pub fn extract_doc_comment(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| {
            match &attr.meta {
                syn::Meta::NameValue(nv) => {
                    match &nv.value {
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(s),
                            ..
                        }) => Some(s.value()),
                        _ => None,
                    }
                }
                _ => None,
            }
        })
        .collect();

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}