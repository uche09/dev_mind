use syn::spanned::Spanned;

use crate::{parser::chunk::{ChunkTokenBound, CodeChunk}, utils::hash::hash_raw_code};

// Boundle tokenizer file for Ahnlich embedding model of choice into project binary.
static TOKENIZER_BYTES: &[u8] = include_bytes!("../assets/jina-embeddings-v2-base-code-tokenizer.json");
pub trait TokenCounter {
    fn count(&self, text: &str) -> anyhow::Result<usize>;
}

pub struct HuggingFaceCounter {
    tokenizer: tokenizers::Tokenizer,
}

impl HuggingFaceCounter {
    pub fn from_embedded() -> anyhow::Result<Self> {
        let tokenizer = tokenizers::Tokenizer::from_bytes(TOKENIZER_BYTES)
            .map_err(|e| anyhow::anyhow!("failed to load tokenizer: {e}"))?;
        Ok(Self { tokenizer })
    }
}

impl TokenCounter for HuggingFaceCounter {
    fn count(&self, text: &str) -> anyhow::Result<usize> {
        self.tokenizer.encode(text, false)
            .map(|enc| enc.len())
            .map_err(|e| anyhow::anyhow!("Token Encoding Error: {}", e))
    }
}


/// A fallback token counter that uses a general rough estimation for token counting.
pub struct HeuristicTokenCounter;

impl TokenCounter for HeuristicTokenCounter {
    fn count(&self, text: &str) -> anyhow::Result<usize> {
        Ok((text.len() as f32 / 3.0).ceil() as usize)
    }
}


pub struct ChunkSplitter<'a> {
    token_counter: &'a dyn TokenCounter,
    chunk_token_bound: ChunkTokenBound,
}

impl<'a> ChunkSplitter<'a> {
    pub fn new(token_counter: &'a dyn TokenCounter, token_bound_config: ChunkTokenBound) -> Self {
        Self{
            token_counter,
            chunk_token_bound: token_bound_config,
        }
    }
    /// Split an oversized `CodeChunk` into sub-chunks within the set token bound for a CodeChunk.
    pub fn split(&self, chunk: CodeChunk) -> anyhow::Result<Vec<CodeChunk>> {
        let token_count = self.token_counter.count(&chunk.build_embedding_text())?;

        if token_count <= self.chunk_token_bound.max_token || chunk.raw_code.is_empty() {
            return Ok(vec![chunk]);
        }
        
        match self.structural_split(&chunk)?{
            Some(sub_chunks) => Ok(sub_chunks),
            None => self.line_window_split(&chunk),
        }
    }

    /// A **syntax (structure)-aware** code splitter for oversized `CodeChunk`s
    pub fn structural_split(&self, chunk: &CodeChunk) -> anyhow::Result<Option<Vec<CodeChunk>>> {
        // wrap chunk code in a throwaway function
        let wrapped = format!("fn __devmind_wrapper__() {{\n{}\n}}", chunk.raw_code);
        let parsed_fn: syn::ItemFn = syn::parse_str(&wrapped)?;

        // extract all code statements from this chunk
        let statements = &parsed_fn.block.stmts;
        if statements.len() < 2 { 
            return Ok(Some(self.line_window_split(chunk)?))
         }

        let raw_lines: Vec<&str> = chunk.raw_code.lines().collect();

        let stmt_ranges: Vec<(usize, usize)> = statements.iter().map(|s| {
            let span = s.span();
            let start = span.start().line.saturating_sub(2); // Undo 1-indexing in code-line numbering and undo wrapper line
            let end = span.end().line.saturating_sub(2).min(raw_lines.len().saturating_sub(1));
            (start, end)
        }).collect();

        let mut groups: Vec<(usize, usize)> = vec![];

        for &(new_stmt_start, new_stmt_end) in &stmt_ranges {
            match groups.last() {
                None => groups.push((new_stmt_start, new_stmt_end)),
                Some(&(group_start, _)) => {
                    let candidate_code = raw_lines[group_start..=new_stmt_end].join("\n");
                    let candidate_chunk = self.derive_subchunk(chunk, (0, 0), &candidate_code, None);
                    let candidate_tokens = self.token_counter.count(&candidate_chunk.build_embedding_text())?;

                    // if new statement pushes group out of the token bound, start a new group
                    // as long as new statement is NOT on the same line with the last group (group_start != new_stmt_start)
                    if candidate_tokens > self.chunk_token_bound.max_token && group_start != new_stmt_start {
                        groups.push((new_stmt_start, new_stmt_end)); // start new group with new statement
                    } else {
                        // Note new statement may cause an over budget, but is IGNORED added to last group if new statement is
                        // on the same line (group_start == new_stmt_start) instead of starting a new group with duplicated lines.
                        // Duplicated line will affect code slice ownership from overlapping index.
                        let last = groups.last_mut().unwrap();
                        last.1 = new_stmt_end; // update last group with new statement
                    }
                }
            }
        }

        let sub_chunks: Vec<CodeChunk> = groups.into_iter().enumerate()
            .map(|(i, (start, end))| {
                let code = raw_lines[start..=end].join("\n");
                self.derive_subchunk(chunk, (start, end), &code, Some(i))
            })
            .collect();

        let mut final_chunks = Vec::new();
        for sub_chunk in sub_chunks {
            let tokens = self.token_counter.count(&sub_chunk.build_embedding_text())?;
            if tokens > self.chunk_token_bound.max_token {
                final_chunks.extend(self.line_window_split(&sub_chunk)?);
            } else {
                final_chunks.push(sub_chunk);
            }
        }

        Ok(Some(final_chunks))
    }

    /// Create a new `CodeChunk` that inherits the ID (file_path, item_kind, comment etc.) of the parent chunk.
    /// Sub-chunks are uniquely ID-ed by the index prepended to the inherited item
    fn derive_subchunk(&self, parent: &CodeChunk, scope: (usize, usize), code: &str, index: Option<usize>) -> CodeChunk {
        let comment_lines = parent.comments.clone().unwrap_or_default();
        let comment_lines = comment_lines.lines().collect::<Vec<_>>();
        let lines = comment_lines.len();
        let half = if lines > 1 { lines / 2 } else {0};

        let sub_comments = if index.is_none() {
            "".to_string()
        }else if index == Some(0) {
            comment_lines[0..half].join("\n")
        } else {
            comment_lines[half..].join("\n")
        };

        CodeChunk {
            file_path: parent.file_path.clone(),
            item_name: format!("{}#part{}", parent.item_name, index.unwrap_or_default()),
            raw_code: code.to_string(),
            doc_comment: if index == Some(0) { parent.doc_comment.clone() } else { None },
            comments: if parent.comments.is_some() { Some(sub_comments) } else { None },
            kind: parent.kind.clone(),

            // sub chunk is a subset of parent chunk, hence its code lines are relative to the parent.
            // the arithmetics below is done to preserve the original code line parsed from the file in the splitted chunks
            start_line: parent.start_line.saturating_add(scope.0).min(parent.end_line),
            end_line: parent.start_line.saturating_add(scope.1).min(parent.end_line),
            content_hash: hash_raw_code(code),
        }
    }

    /// A syntax (structure)-unaware code splitter for oversized `CodeChunk`s. 
    /// 
    /// Uses a split window that starts at half the total line of code with a configured overlapping lines
    /// for each window, and gradually adjust window if still over budget.
    fn line_window_split(&self, chunk: &CodeChunk) -> anyhow::Result<Vec<CodeChunk>> {
        let code_lines = chunk.raw_code.lines().collect::<Vec<_>>();
        if code_lines.is_empty() { return Ok(vec![chunk.clone()]); }

        let mut window = code_lines.len().div_ceil(2).max(1);

        loop {
            let mut i = 0;
            let mut splitted_code = vec![];
            while i < code_lines.len() {
                let start = i.saturating_sub(self.chunk_token_bound.overlap_lines);
                let end = (i + window).min(code_lines.len() -1);
                let code = code_lines[start..=end].join("\n");
                splitted_code.push((code, (start, end)));
                i += window +1; // Exclude the included =end index from the next window.
            }

            let sub_chunks = splitted_code.into_iter().enumerate()
                .map(|(i, (s, scope))| {
                    self.derive_subchunk(chunk, scope, &s, Some(i))
                }).collect::<Vec<CodeChunk>>();

            let token_counts = sub_chunks.iter()
                .map(|c| self.token_counter.count(&c.build_embedding_text()))
                .collect::<Result<Vec<usize>, anyhow::Error>>()?;

            let all_within_bound = token_counts.iter()
                .all(|&tokens| tokens <= self.chunk_token_bound.max_token);

            if all_within_bound || window <= 1 { return Ok(sub_chunks) }

            window = (window / 2).max(1);
        }
    }
}






#[cfg(test)]
mod tests {
    use crate::parser::chunk::{self, ChunkKind};

use super::*;

    // -------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------

    /// A counter with predictable, exact output: one token per
    /// whitespace-separated word. HeuristicTokenCounter's char/3.0 ratio
    /// is fine for production, but it makes boundary tests fuzzy (you
    /// can't easily construct a string that's "exactly 50 tokens").
    /// This counter makes boundary math exact so tests can assert on
    /// precise counts instead of "roughly around the limit."
    struct WordCountTokenCounter;

    impl TokenCounter for WordCountTokenCounter {
        fn count(&self, text: &str) -> anyhow::Result<usize> {
            Ok(text.split_whitespace().count())
        }
    }

    fn make_chunk(raw_code: &str, kind: ChunkKind) -> CodeChunk {
        CodeChunk {
            file_path: "src/services/ai_service.rs".into(),
            item_name: "handle_request".into(),
            start_line: 1,
            kind,
            end_line: raw_code.lines().count(),
            raw_code: raw_code.to_string(),
            doc_comment: Some("Handles an incoming request.".into()),
            comments: Some(String::new()),
            content_hash: String::new(),
        }
    }

    fn config(max_token: usize, overlap_lines: usize) -> ChunkTokenBound {
        ChunkTokenBound { max_token, overlap_lines }
    }

    // -------------------------------------------------------------
    // 1. Identity path — chunk already fits
    // -------------------------------------------------------------

    #[test]
    fn small_chunk_is_not_split() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, chunk::ChunkTokenBound::default());
        let chunk = make_chunk("fn foo() {\n    println!(\"hi\");\n}", ChunkKind::Function);

        let result = splitter.split(chunk.clone())?;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].raw_code, chunk.raw_code);
        Ok(())
    }

    #[test]
    fn chunk_exactly_at_the_boundary_is_not_split() -> anyhow::Result<()>{
        // build_embedding_text() wraps raw_code in a template, so we can't
        // predict the *exact* word count of the finished text by hand here.
        // Instead: measure it first, then set max_tokens to that exact
        // value, proving the boundary is inclusive (<=), not exclusive (<).
        let counter = WordCountTokenCounter;
        let chunk = make_chunk("let x = 1;\nlet y = 2;", ChunkKind::Function);
        let exact_tokens = counter.count(&chunk.build_embedding_text())?;

        let splitter = ChunkSplitter::new(&counter, config(exact_tokens, 0));
        let result = splitter.split(chunk)?;

        assert_eq!(result.len(), 1, "a chunk exactly at the limit should pass through unsplit");
        Ok(())
    }

    #[test]
    fn chunk_one_token_over_the_boundary_is_split() -> anyhow::Result<()>{
        let counter = WordCountTokenCounter;
        let chunk = make_chunk(
            "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;\nlet f = 6;",
            ChunkKind::Function
        );
        let exact_tokens = counter.count(&chunk.build_embedding_text())?;

        let splitter = ChunkSplitter::new(&counter, config(exact_tokens - 1, 1));
        let result = splitter.split(chunk)?;

        assert!(result.len() > 1, "one token over the limit should trigger a split");
        Ok(())
    }

    // -------------------------------------------------------------
    // 2. Structural splitting (multi-statement code)
    // -------------------------------------------------------------

    #[test]
    fn oversized_multi_statement_chunk_is_split_within_bound() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let cfg = config(58, 2);
        let splitter = ChunkSplitter::new(&counter, cfg.clone());

        let big_body: String = (0..200)
            .map(|i| format!("    let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&big_body, ChunkKind::Function);

        let result = splitter.split(chunk)?;

        assert!(result.len() > 1, "expected the chunk to be split");
        for sub in &result {
            let tokens = counter.count(&sub.build_embedding_text())?;
            assert!(
                tokens <= cfg.max_token,
                "sub-chunk exceeded bound: {tokens} tokens (limit {})",
                cfg.max_token
            );
        }
        Ok(())
    }

    #[test]
    fn structural_split_keeps_each_subchunk_as_valid_statements() -> anyhow::Result<()>{
        // Every sub-chunk's raw_code should itself be parseable as a
        // sequence of statements, proving the split landed on statement
        // boundaries rather than cutting mid-expression.
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, config(30, 0));

        let body: String = (0..50)
            .map(|i| format!("    let val{i} = compute({i});"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&body, ChunkKind::Function);

        let result = splitter.split(chunk)?;

        for sub in &result {
            let wrapped = format!("fn __check__() {{\n{}\n}}", sub.raw_code);
            assert!(
                syn::parse_str::<syn::ItemFn>(&wrapped).is_ok(),
                "sub-chunk is not valid Rust statements:\n{}",
                sub.raw_code
            );
        }
        Ok(())
    }

    #[test]
    fn single_statement_chunk_skips_structural_split() -> anyhow::Result<()>{
        // Only one statement means structural_split's `stmts.len() < 2`
        // guard returns None immediately, so this must fall through to
        // line_window_split instead, and still respect the bound.
        let counter = WordCountTokenCounter;
        let long_match = format!(
            "match n {{\n{}\n_ => 0,\n}}",
            (0..100).map(|i| format!("    {i} => {i} * 2,")).collect::<Vec<_>>().join("\n")
        );
        let chunk = make_chunk(&long_match, ChunkKind::Function);
        let splitter = ChunkSplitter::new(&counter, config(40, 3));

        let result = splitter.split(chunk)?;

        assert!(result.len() > 1, "single oversized statement should still be split by line window");
        for sub in &result {
            let tokens = counter.count(&sub.build_embedding_text())?;
            assert!(tokens <= 40, "sub-chunk exceeded bound: {tokens}");
        }
        Ok(())
    }

    #[test]
    fn statements_sharing_one_source_line_are_never_split_apart() -> anyhow::Result<()> {
        let counter = WordCountTokenCounter;
        let code = [
            "let x = 10;",
            "let y = 10; let z = 10;",
            "let a = 1; let b = 2; let c = 3; let d = 4; let e = 5;",
            "let f = 10; let g = 10; let h = 10;",
            "let i = 10; let j = 10; let k = 10; let l = 10;",
            "let m = 10; let n = 10; let o = 10; let p = 10; let q = 10; let r = 10;",
            "let s = 10; let t = 10; let u = 10; let v = 10; let w = 10; let xx = 10; let yy = 10;",
            "let aa = {\n(1..8).sum()\n}; let bb = {\n(1..10).sum()\n};\nlet cc = {\n(1..11).sum()\n}; let dd = {\n(1..8).sum()\n}; let ee = {\n(1..8).sum()\n};"
        ].join("\n");

        let chunk = make_chunk(&code, ChunkKind::Function);

        // force an artificially tiny bound so the packer WANTS to split
        let splitter = ChunkSplitter::new(&counter, config(27, 0));
        let result = splitter.split(chunk)?;

        // whatever comes out, no line should be duplicated across
        // sub-chunks with a start line that overlaps another group's range
        for pair in result.windows(2) {
            let first = &pair[0];
            let second = &pair[1];

            // assert start/end ranges never overlap here
            assert!(
                first.end_line < second.start_line,
                "Line overlap detected! Chunk '{}' (lines {}-{}) overlaps with '{}' (lines {}-{})",
                first.item_name, first.start_line, first.end_line,
                second.item_name, second.start_line, second.end_line
            )
        }
        Ok(())
    }
    
    // -------------------------------------------------------------
    // 3. The "recursing on stragglers" path
    // -------------------------------------------------------------

    #[test]
    fn oversized_single_statement_among_small_ones_is_demoted_to_line_window() -> anyhow::Result<()> {
        // Mix of many tiny statements plus one enormous one-liner. The
        // packing loop's `current.len() > 1` guard will let the huge
        // statement through as its own oversized group; the post-pass
        // must then catch it and hand it to line_window_split.
        let counter = WordCountTokenCounter;
        let long_match = format!(
            "match n {{\n{}\n_ => 0,\n}}",
            (0..100).map(|i| format!("    {i} => {i} * 2,")).collect::<Vec<_>>().join("\n")
        );
        let body = format!("let a = 1;\nlet b = 2;\n{long_match}\nlet c = 3;");
        let chunk = make_chunk(&body, ChunkKind::Function);
        let cfg = config(30, 2);
        let splitter = ChunkSplitter::new(&counter, cfg.clone());
        let result = splitter.split(chunk)?;

        assert!(result.len() > 2, "the huge statement should have been broken down further");
        for sub in &result {
            let tokens = counter.count(&sub.build_embedding_text())?;
            assert!(tokens <= cfg.max_token, "straggler sub-chunk exceeded bound: {tokens}");
        }
        Ok(())
    }

    // -------------------------------------------------------------
    // 4. Non-statement code → structural_split returns None
    // -------------------------------------------------------------

    #[test]
    fn non_statement_code_falls_back_to_line_window_split() -> anyhow::Result<()> {
        // A struct definition isn't a sequence of statements, so wrapping
        // it in `fn __devmind_wrapper__() { ... }` won't parse. This must
        // fall straight to line_window_split.
        let counter = WordCountTokenCounter;
        let big_struct: String = std::iter::once("struct Big {".to_string())
            .chain((0..100).map(|i| format!("    field_{i}: u32,")))
            .chain(std::iter::once("}".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&big_struct, ChunkKind::Struct);

        let splitter = ChunkSplitter::new(&counter, config(30, 2));
        let result = splitter.split(chunk)?;

        assert!(result.len() > 1);
        for sub in &result {
            let tokens = counter.count(&sub.build_embedding_text())?;
            assert!(tokens <= 30, "sub-chunk exceeded bound: {tokens}");
        }
        Ok(())
    }

    // -------------------------------------------------------------
    // 5. Line-window overlap behavior
    // -------------------------------------------------------------

    #[test]
    fn line_window_split_repeats_overlap_lines_between_windows() -> anyhow::Result<()> {
        let counter = WordCountTokenCounter;
        let lines: Vec<String> = (0..40).map(|i| format!("line_{i}();")).collect();
        let chunk = make_chunk(&lines.join("\n"), ChunkKind::Function);

        let splitter = ChunkSplitter::new(&counter, config(15, 3));
        let result = splitter.line_window_split(&chunk)?;

        assert!(result.len() > 1, "expected more than one window");

        // The last `overlap_lines` lines of window N should reappear as
        // the first lines of window N+1.
        for pair in result.windows(2) {
            let prev_lines: Vec<&str> = pair[0].raw_code.lines().collect();
            let next_lines: Vec<&str> = pair[1].raw_code.lines().collect();
            let overlap_count = 3.min(prev_lines.len()).min(next_lines.len());

            let prev_tail = &prev_lines[prev_lines.len() - overlap_count..];
            let next_head = &next_lines[..overlap_count];

            assert_eq!(
                prev_tail, next_head,
                "expected overlapping lines between consecutive windows"
            );
        }
        Ok(())
    }

    #[test]
    fn no_source_lines_are_lost_across_a_split() -> anyhow::Result<()> {
        // Correctness property: every original line must appear in at
        // least one sub-chunk. Overlap means some lines legitimately
        // appear twice, but none should vanish.
        let counter = WordCountTokenCounter;
        let lines: Vec<String> = (0..60).map(|i| format!("op_{i}();")).collect();
        let chunk = make_chunk(&lines.join("\n"), ChunkKind::Function);

        let splitter = ChunkSplitter::new(&counter, config(10, 2));
        let result = splitter.line_window_split(&chunk)?;

        let covered: std::collections::HashSet<&str> = result
            .iter()
            .flat_map(|c| c.raw_code.lines())
            .collect();

        for line in &lines {
            assert!(
                covered.contains(line.as_str()),
                "line `{line}` was dropped during splitting"
            );
        }
        Ok(())
    }

    // -------------------------------------------------------------
    // 6. derive_subchunk metadata behavior
    // -------------------------------------------------------------

    #[test]
    fn only_first_subchunk_keeps_the_doc_comment() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, config(30, 1));

        let big_struct: String = std::iter::once("struct Big {".to_string())
            .chain((0..60).map(|i| format!("    field_{i}: u32,")))
            .chain(std::iter::once("}".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&big_struct, ChunkKind::Struct);

        let result = splitter.split(chunk.clone())?;

        assert!(result.len() > 1);
        assert_eq!(result[0].doc_comment, chunk.doc_comment);
        for sub in &result[1..] {
            assert!(sub.doc_comment.is_none(), "only the first sub-chunk should keep the doc comment");
        }
        Ok(())
    }

    #[test]
    fn each_subchunk_gets_a_unique_content_hash() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, config(30, 1));

        let big_struct: String = std::iter::once("struct Big {".to_string())
            .chain((0..60).map(|i| format!("    field_{i}: u32,")))
            .chain(std::iter::once("}".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&big_struct, ChunkKind::Struct);

        let result = splitter.split(chunk)?;
        let hashes: std::collections::HashSet<&str> =
            result.iter().map(|c| c.content_hash.as_str()).collect();

        assert_eq!(
            hashes.len(),
            result.len(),
            "every sub-chunk should have a distinct content_hash"
        );
        Ok(())
    }

    #[test]
    fn subchunk_item_names_are_suffixed_and_ordered() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, config(30, 1));

        let big_struct: String = std::iter::once("struct Big {".to_string())
            .chain((0..60).map(|i| format!("    field_{i}: u32,")))
            .chain(std::iter::once("}".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&big_struct, ChunkKind::Struct);

        let result = splitter.split(chunk)?;

        for (i, sub) in result.iter().enumerate() {
            assert_eq!(sub.item_name, format!("handle_request#part{i}"));
        }
        Ok(())
    }

    // -------------------------------------------------------------
    // 7. Edge cases
    // -------------------------------------------------------------

    #[test]
    fn empty_raw_code_is_returned_unchanged() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, config(5, 1));
        let chunk = make_chunk("", ChunkKind::Test);

        let result = splitter.split(chunk)?;

        assert_eq!(result.len(), 1);
        assert!(result[0].raw_code.is_empty());
        Ok(())
    }

    #[test]
    fn single_line_chunk_cannot_be_split_further() -> anyhow::Result<()> {
        // With only one line, line_window_split has nothing smaller to
        // fall back to; it must still return at least that one chunk
        // rather than panicking or looping.
        let counter = WordCountTokenCounter;
        let chunk = make_chunk("let x = very_long_function_call_with_many_words_in_it();", ChunkKind::Function);
        let splitter = ChunkSplitter::new(&counter, config(1, 0));

        let result = splitter.line_window_split(&chunk)?;

        assert_eq!(result.len(), 1, "a single line can't be split below one window");
        Ok(())
    }

    #[test]
    fn very_generous_bound_never_splits_realistic_code() -> anyhow::Result<()> {
        let counter = HeuristicTokenCounter;
        let splitter = ChunkSplitter::new(&counter, ChunkTokenBound::default());

        let body: String = (0..500)
            .map(|i| format!("    let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = make_chunk(&body, ChunkKind::Function);

        // Default config's 7300-token bound should comfortably fit this.
        let result = splitter.split(chunk)?;
        assert_eq!(result.len(), 1);
        Ok(())
    }
}
