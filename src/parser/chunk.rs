use std::fmt::Display;
use syn::{spanned::Spanned, visit::{self, Visit}};
use crate::utils::{helper, hash::hash_raw_code};


pub enum ChunkKind {
    Function,
    Struct,
    Trait,
    Enum,
    Method,
    TraitMethod,
    Test,
}

impl Display for ChunkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enum => write!(f, "Enum"),
            Self::Function => write!(f, "Function"),
            Self::Method => write!(f, "Method"),
            Self::Struct => write!(f, "Struct"),
            Self::Trait => write!(f, "Trait"),
            Self::TraitMethod => write!(f, "Trait Method"),
            Self::Test => write!(f, "Test function"),
        }
    }
}

pub struct CodeChunk {
    pub file_path: String,
    pub kind: ChunkKind,
    pub item_name: String,
    // pub start_line: usize,
    // pub end_line: usize,
    pub doc_comment: Option<String>,
    pub raw_code: String,
    pub content_hash: String,
}

impl CodeChunk {
    pub fn build_embedding_text(&self) -> String {
        format!(
            "File: {}\n{}: {}\n{}\n\n{}",
            self.file_path,
            self.kind,
            self.item_name,
            self.doc_comment.clone().unwrap_or_default(),
            self.raw_code
        )
    }
}

impl Display for CodeChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.build_embedding_text())
    }
}

pub struct ChunkVisitor<'a> {
    pub lines: &'a [&'a str],
    pub file_path: &'a str,
    pub is_test_mod: bool, // State field to identify visit into test modules #[cfg[test]]
    pub chunks: Vec<CodeChunk>,
}

impl<'a> ChunkVisitor<'a> {
    fn push(&mut self, span: proc_macro2::Span, name: &str, kind: ChunkKind, attrs: &[syn::Attribute]) {
        let (start, end) = (span.start().line, span.end().line);
        let raw_code = self.lines[start -1..end].join("\n");
        self.chunks.push(CodeChunk { 
            file_path: self.file_path.into(), 
            kind,
            item_name: name.into(),
            // start_line: start, end_line: end, 
            doc_comment: helper::extract_doc_comment(attrs),
            content_hash: hash_raw_code(&raw_code),
            raw_code,
        });
    }
}

impl<'a> Visit<'a> for ChunkVisitor<'a> {
    fn visit_item_fn(&mut self, i: &'a syn::ItemFn) {
        // Mark all functions in #[cfg(test)] test module as ChunkKind::Test
        // including helper functions without the #[test] attribute.
        let kind = if self.is_test_mod {ChunkKind::Test} else {ChunkKind::Function};
        self.push(i.span(), &i.sig.ident.to_string(), kind, &i.attrs);
    }

    fn visit_item_struct(&mut self, i: &'a syn::ItemStruct) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Struct, &i.attrs);
    }

    fn visit_item_trait(&mut self, i: &'a syn::ItemTrait) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Trait, &i.attrs);
    }

    fn visit_item_enum(&mut self, i: &'a syn::ItemEnum) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Enum, &i.attrs);
    }

    fn visit_impl_item_fn(&mut self, i: &'a syn::ImplItemFn) {
        self.push(i.span(), &i.sig.ident.to_string(), ChunkKind::Method, &i.attrs);
        visit::visit_impl_item_fn(self, i);
    }

    fn visit_trait_item_fn(&mut self, i: &'a syn::TraitItemFn) {
        self.push(i.span(), &i.sig.ident.to_string(), ChunkKind::TraitMethod, &i.attrs);
        visit::visit_trait_item_fn(self, i);
    }

    fn visit_item_mod(&mut self, i: &'a syn::ItemMod) {
        // flip one State field when in a test module
        self.is_test_mod = helper::is_cfg_test_mod(&i.attrs);

        visit::visit_item_mod(self, i);
        self.is_test_mod = false; // revert State field
    }
}



#[cfg(test)]
mod tests {
    use super::*;


    fn parse_str(src: &str) -> anyhow::Result<Vec<CodeChunk>> {
        let lines: Vec<&str> = src.lines().collect();
        let mut visitor = ChunkVisitor {
            lines: &lines,
            file_path: "/",
            is_test_mod: false,
            chunks: vec![]
        };

        let file: syn::File = syn::parse_str(src)?;
        visitor.visit_file(&file);
        Ok(visitor.chunks)
    }

    #[test]
    fn function_span_includes_full_body_with_nested_braces() -> anyhow::Result<()> {
        let src = r#"
        fn outer() -> i32 {
            let x = if true { 1 } else { 2 };
            match x {
                1 => { 10 }
                _ => { 20 }
            }
        }
        fn next_fn() {}
        "#;

        let chunks = parse_str(src)?;

        let outer = chunks.iter().find(|&c| c.item_name == "outer").unwrap();
        assert!(outer.raw_code.contains("match x"));
        assert!(outer.raw_code.trim_end().ends_with('}'));
        assert!(!outer.raw_code.contains("next_fn")); // didn't bleed into the next item
        Ok(())
    }

    #[test]
    fn closure_inside_function_does_not_become_its_own_chunk() -> anyhow::Result<()> {
        let src = r#"
    fn has_closure() {
        let f = |x: i32| x + 1;
        f(2);
    }
    "#;
        let chunks = parse_str(src)?;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].item_name, "has_closure");
        Ok(())
    }

    #[test]
    fn impl_method_is_tagged_as_method_not_function() -> anyhow::Result<()> {
        let src = r#"
            struct Foo;
            impl Foo {
                fn bar(&self) -> i32 { 1 }
            }
        "#;

        let chunks = parse_str(src)?;
        let bar = chunks.iter().find(|c| c.item_name == "bar").unwrap();
        assert!(matches!(bar.kind, ChunkKind::Method));
        assert_eq!(chunks.len(), 2); // struct Foo + method bar, nothing extra, nothing missing
        Ok(())
    }

    #[test]
    fn invalid_rust_file_returns_err_not_panic() {
        let result = parse_str(
            "fn bad_code( {"
        );
        assert!(result.is_err());
    }
}