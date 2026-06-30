pub mod traverser;
pub mod chunk;

use chunk::{CodeChunk, ChunkVisitor};
use syn::visit::Visit;


pub fn parse_file(path: &str) -> anyhow::Result<Vec<CodeChunk>> {
    let source = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = source.lines().collect();
    let mut visitor = ChunkVisitor {
        lines: &lines,
        file_path: path,
        is_test_mod: false,
        chunks: vec![]
    };

    let file = syn::parse_file(&source)?;
    visitor.visit_file(&file);
    Ok(visitor.chunks)
}