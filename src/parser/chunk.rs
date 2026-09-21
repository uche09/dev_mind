use crate::utils::{hash::hash_raw_code, helper};
use std::fmt::Display;
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
};

#[derive(Clone)]
pub struct ChunkTokenBound {
    pub max_token: usize,
    pub overlap_lines: usize,
}

impl Default for ChunkTokenBound {
    fn default() -> Self {
        Self {
            max_token: 7000,
            overlap_lines: 10,
        }
    }
}

#[derive(Clone)]
/// Scopes extractable Rust construct and structures to these more commonly used
pub enum ChunkKind {
    Function,
    Struct,
    Trait, // Default trait methods are captured in Trait Definition
    Enum,
    Method(String),
    TraitMethod((String, String)), // (trait_name, type_implemented)
    Test,
}

impl Display for ChunkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enum => write!(f, "Enum"),
            Self::Function => write!(f, "Function"),
            Self::Method(type_) => write!(f, "Method(impl {type_})"),
            Self::Struct => write!(f, "Struct"),
            Self::Trait => write!(f, "Trait"),
            Self::TraitMethod(meta_) => write!(f, "Trait Method(impl {} for {})", meta_.0, meta_.1),
            Self::Test => write!(f, "Test function"),
        }
    }
}

#[derive(Clone)]
/// Representation of a single extracted code construct
pub struct CodeChunk {
    pub file_path: String,
    pub kind: ChunkKind,
    pub item_name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub doc_comment: Option<String>,
    pub comments: Option<String>,
    pub raw_code: String,
    pub content_hash: String,
}

impl CodeChunk {
    /// Format embedding text so it makes sense even to a text embedding model.
    pub fn build_embedding_text(&self) -> String {
        let method_semantic_header = match &self.kind {
            ChunkKind::TraitMethod((trait_, type_)) => {
                format!("Context: Implementation of {trait_} for type {type_}\n")
            }
            ChunkKind::Method(type_) => format!("Context: Inherent method for type {type_}\n"),
            _ => "".to_string(),
        };
        format!(
            "{}File: {}\n{}: {}\nDoc Comment: {}\nComments: \n{}\n\nCode Snippet:\n{}",
            method_semantic_header,
            self.file_path,
            self.kind,
            self.item_name,
            self.doc_comment.clone().unwrap_or_default(),
            self.comments.clone().unwrap_or_default(),
            self.raw_code
        )
    }
}

impl Display for CodeChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.build_embedding_text())
    }
}

/// Represent the current nesting state of the parser
#[derive(Default, Clone)]
pub struct ImplContext {
    pub trait_name: Option<String>,
    pub self_type: String,
}

/// Chunk extractor. Use the `syn` crate to identity rust logical structures and extract them from a rust file.
pub struct ChunkVisitor<'a> {
    pub lines: &'a [&'a str],
    pub file_path: &'a str,
    pub is_test_mod: bool, // State field to identify visit into test modules #[cfg[test]]
    pub current_impl: Option<ImplContext>, // State field to identify trait methods `impl for` and normal methods `impl`
    pub chunks: Vec<CodeChunk>,
}

impl<'a> ChunkVisitor<'a> {
    fn push(
        &mut self,
        span: proc_macro2::Span,
        name: &str,
        kind: ChunkKind,
        attrs: &[syn::Attribute],
    ) {
        let (start, end) = (span.start().line, span.end().line);
        let raw_code = self.lines[start - 1..end].join("\n");
        self.chunks.push(CodeChunk {
            file_path: self.file_path.into(),
            kind,
            item_name: name.into(),
            start_line: start,
            end_line: end,
            doc_comment: helper::extract_doc_comment(attrs),
            comments: helper::extract_regular_comments(&self.lines[start - 1..end]),
            content_hash: hash_raw_code(&raw_code),
            raw_code,
        });
    }

    /// Helper to extract a clean string from syn::Type
    fn type_to_string(ty: &syn::Type) -> String {
        match ty {
            syn::Type::Path(type_path) => {
                let segments: Vec<String> = type_path
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();

                segments.join("::")
            }
            _ => "UnknownType".to_string(),
        }
    }

    /// Helper to extract a clean string from syn::Path
    fn path_to_string(path: &syn::Path) -> String {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        segments.join("::")
    }
}

impl<'a> Visit<'a> for ChunkVisitor<'a> {
    fn visit_item_fn(&mut self, i: &'a syn::ItemFn) {
        // Mark all functions in #[cfg(test)] test module as ChunkKind::Test
        // including helper functions without the #[test] attribute.
        let kind = if self.is_test_mod {
            ChunkKind::Test
        } else {
            ChunkKind::Function
        };
        self.push(i.span(), &i.sig.ident.to_string(), kind, &i.attrs);
    }

    fn visit_item_struct(&mut self, i: &'a syn::ItemStruct) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Struct, &i.attrs);
    }

    // Capturing Trait definition might lead to raw code duplication as `visit_trait_item_fn()`
    // would also capture individual trait function code which are already captured
    // with the trait definition with this function
    fn visit_item_trait(&mut self, i: &'a syn::ItemTrait) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Trait, &i.attrs);
    }

    fn visit_item_enum(&mut self, i: &'a syn::ItemEnum) {
        self.push(i.span(), &i.ident.to_string(), ChunkKind::Enum, &i.attrs);
    }

    fn visit_item_impl(&mut self, i: &'a syn::ItemImpl) {
        // Extract the target Struct/Enum/Primitive name
        let block_name = Self::type_to_string(&i.self_ty);

        // Extract the Trait path if it exists (impl Trait for Struct)
        let trait_name = i
            .trait_
            .as_ref()
            .map(|(_, path, _)| Self::path_to_string(path));

        let previous_context = self.current_impl.take();
        self.current_impl = Some(ImplContext {
            trait_name,
            self_type: block_name,
        });

        visit::visit_item_impl(self, i);

        // restore previous context
        self.current_impl = previous_context;
    }

    fn visit_impl_item_fn(&mut self, i: &'a syn::ImplItemFn) {
        if let Some(ctx) = &self.current_impl {
            let kind = match &ctx.trait_name {
                Some(trait_) => {
                    ChunkKind::TraitMethod((trait_.to_string(), ctx.self_type.to_string()))
                }
                None => ChunkKind::Method(ctx.self_type.to_string()),
            };

            self.push(i.span(), &i.sig.ident.to_string(), kind, &i.attrs);
            visit::visit_impl_item_fn(self, i);
            return;
        }

        self.push(
            i.span(),
            &i.sig.ident.to_string(),
            ChunkKind::Method("".into()),
            &i.attrs,
        );
        visit::visit_impl_item_fn(self, i);
    }

    fn visit_item_mod(&mut self, i: &'a syn::ItemMod) {
        // flip on State field when in a test module
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
            current_impl: None,
            chunks: vec![],
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
        assert!(matches!(&bar.kind, ChunkKind::Method(_type)));
        assert_eq!(chunks.len(), 2); // struct Foo + method bar, nothing extra, nothing missing
        Ok(())
    }

    #[test]
    fn invalid_rust_file_returns_err_not_panic() {
        let result = parse_str("fn bad_code( {");
        assert!(result.is_err());
    }

    #[test]
    fn tags_fn_inside_cfg_test_mod_as_test() -> anyhow::Result<()> {
        let src = r#"
            #[cfg(test)]
            mod tests {
                fn helper_function() {}
                
                #[test]
                fn nested_test() { assert!(true); }
            }
        "#;
        let chunks = parse_str(src)?;
        let nested = chunks
            .iter()
            .find(|c| c.item_name == "nested_test")
            .unwrap();
        let helper = chunks
            .iter()
            .find(|c| c.item_name == "helper_function")
            .unwrap();
        assert!(matches!(nested.kind, ChunkKind::Test));
        assert!(matches!(helper.kind, ChunkKind::Test));
        Ok(())
    }

    #[test]
    fn does_not_misfire_on_unrelated_cfg_attribute() -> anyhow::Result<()> {
        // is_cfg_test_mod must not get fooled by a non-"test" cfg condition
        let src = r#"
            #[cfg(target_os = "linux")]
            fn linux_only() {}
        "#;
        let chunks = parse_str(src)?;
        let chunk = chunks.iter().find(|c| c.item_name == "linux_only").unwrap();
        assert!(!matches!(chunk.kind, ChunkKind::Test));
        Ok(())
    }

    #[test]
    fn detects_correct_cfg_test_mod_attribute() -> anyhow::Result<()> {
        let item: syn::ItemMod = syn::parse_str("#[cfg(test)] mod tests {}")?;
        assert!(helper::is_cfg_test_mod(&item.attrs));

        let item2: syn::ItemMod = syn::parse_str(r#"#[cfg(feature = "x")] mod foo {}"#)?;
        assert!(!helper::is_cfg_test_mod(&item2.attrs));
        Ok(())
    }

    #[test]
    fn captures_trait_definition_and_its_methods() -> anyhow::Result<()> {
        let src = r#"
            trait Retryable {
                fn max_attempts(&self) -> u32;
                fn retry(&self) -> bool {
                    self.max_attempts() > 0
                }
            }
        "#;
        let chunks = parse_str(src)?;

        let trait_chunk = chunks.iter().find(|c| c.item_name == "Retryable").unwrap();
        assert!(matches!(trait_chunk.kind, ChunkKind::Trait));
        assert!(trait_chunk.raw_code.contains("self.max_attempts() > 0"));
        Ok(())
    }
}
