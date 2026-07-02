use syn::{Attribute, Expr, ExprLit, Lit, Meta};

pub fn extract_doc_comment(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| {
            match &attr.meta {
                Meta::NameValue(nv) => {
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

pub fn extract_regular_comments<'a>(lines: &[&'a str]) -> Option<String> {
    let comments: Vec<String> = lines.iter()
        .filter_map(|line| line.trim().contains("//").then_some(line.to_string()))
        .collect();
    
    if comments.is_empty() {
        None
    } else {
        Some(comments.join("\n"))
    }
}

pub fn is_cfg_test_mod(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr.parse_args::<Meta>()
                .map(|m| m.path().is_ident("test"))
                .unwrap_or(false)
    })
}