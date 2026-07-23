use rustc_lexer::{TokenKind, tokenize, FrontmatterAllowed};
use syn::{Attribute, Expr, ExprLit, Lit, Meta};

pub fn extract_doc_comment(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .collect();

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}

pub fn extract_regular_comments(lines: &[&str]) -> Option<String> {
    let input = lines.join("\n");
    let mut comments = Vec::new();
    let mut pos = 0;

    for token in tokenize(&input, FrontmatterAllowed::No) {
        let text = &input[pos..pos + token.len as usize];

        match token.kind {
            TokenKind::LineComment { doc_style: None } => {
                comments.push(text.trim().to_string());

            }
            TokenKind::BlockComment { doc_style: None, terminated: true } => {
                comments.push(text.trim().to_string());

            }
            TokenKind::BlockComment { doc_style: None, terminated: false } => {
                // Delibrately ignoring unclosed comments
            }
            _ => {}
        }

        pos += token.len as usize;
    }

    if comments.is_empty() {
        None
    } else {
        Some(comments.join("\n"))
    }
}

pub fn is_cfg_test_mod(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .map(|m| m.path().is_ident("test"))
                .unwrap_or(false)
    })
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_line_comment() {
        let src = "// just a regular comment\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(
            extract_regular_comments(&src),
            Some("// just a regular comment".to_string())
        );
    }

    #[test]
    fn excludes_outer_doc_line_comment() {
        let src = "/// this documents foo\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }

    #[test]
    fn excludes_inner_doc_line_comment() {
        let src = "fn foo() {\n    //! documents foo from inside\n}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }

    #[test]
    fn extracts_plain_block_comment() {
        let src = "/* just a block comment */\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(
            extract_regular_comments(&src),
            Some("/* just a block comment */".to_string())
        );
    }

    #[test]
    fn excludes_outer_doc_block_comment() {
        let src = "/** documents foo */\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }

    #[test]
    fn excludes_inner_doc_block_comment() {
        let src = "fn foo() {\n    /*! documents foo from inside */\n}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }

    #[test]
    fn empty_block_comment_is_not_treated_as_doc() {
        // "/**/" starts with "/**" as a raw substring but is NOT a doc comment
        let src = "/**/\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(
            extract_regular_comments(&src),
            Some("/**/".to_string())
        );
    }

    #[test]
    fn triple_asterisk_block_comment_is_not_treated_as_doc() {
        // "/***...*/" is explicitly not a doc comment, mirrors "////" for line comments
        let src = "/*** not a doc comment ***/\nfn foo() {}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(
            extract_regular_comments(&src),
            Some("/*** not a doc comment ***/".to_string())
        );
    }

    #[test]
    fn ignores_double_slash_inside_string_literal() {
        let src = r#"let url = "https://example.com";"#;
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }

    #[test]
    fn extracts_nested_comment_inside_function_body() {
        let src = "fn foo() {\n    // a regular note\n    let x = 1;\n}";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(
            extract_regular_comments(&src),
            Some("// a regular note".to_string())
        );
    }

    #[test]
    fn no_comments_returns_none() {
        let src = "fn foo() { let x = 1; }";
        let src: Vec<&str> = src.lines().collect();
        assert_eq!(extract_regular_comments(&src), None);
    }
}