// Requires a running Ahnlich instance:
//     docker compose -f ahnlich-docker-compose.yml up
//
// Tests one specific hypothesis: does a batch's cost depend on the SUM of
// its members' token counts, or on (batch_len * longest_member)? If Ahnlich
// pads every sequence in a `Set` batch to match the longest one before
// running inference, two batches with a near-identical token SUM should
// still take very different amounts of time if their size DISTRIBUTION
// differs, one uniform, one skewed.
//

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use devmind::embeddings::ahnlich::CodeIndex;
use devmind::parser::chunk::{ChunkKind, CodeChunk};
use devmind::parser::tokenizer::{HuggingFaceCounter, TokenCounter};
use std::sync::Arc;
use std::time::Duration;

const AHNLICH_ADDR: &str = "localhost:1370";
const STORE: &str = "devmind_bench_padding";

/// Builds a `CodeChunk` whose `build_embedding_text()` measures at
/// approximately `target_tokens`, by growing a body of filler statements
/// until the real tokenizer count is close enough. Exact token counts
/// aren't achievable by hand (tokenization isn't 1:1 with words or
/// characters), so this accepts anything within `tolerance` tokens of
/// the target rather than chasing an exact number.
fn chunk_with_approx_tokens(
    counter: &dyn TokenCounter,
    target_tokens: usize,
    tolerance: usize,
    label: &str,
) -> CodeChunk {
    let mut line_count = (target_tokens / 4).max(1); // rough starting guess

    loop {
        let body: String = (0..line_count)
            .map(|i| format!("    let filler_value_{i} = {i} * 2 + 1;"))
            .collect::<Vec<_>>()
            .join("\n");

        let mut chunk = CodeChunk {
            file_path: "bench/synthetic.rs".into(),
            kind: ChunkKind::Function,
            item_name: label.into(),
            start_line: 1,
            end_line: line_count,
            doc_comment: None,
            comments: None,
            raw_code: body,
            content_hash: String::new(),
            token_count: None,
        };

        let measured = counter.count(&chunk.build_embedding_text()).unwrap();

        if measured.abs_diff(target_tokens) <= tolerance {
            chunk.token_count = Some(measured);
            return chunk;
        }

        // Adjust line count proportionally and try again. This converges
        // in a handful of iterations since embedding text length scales
        // roughly linearly with line count for this filler pattern.
        if measured == 0 {
            line_count += 1;
        } else {
            let ratio = target_tokens as f64 / measured as f64;
            line_count = ((line_count as f64) * ratio).ceil().max(1.0) as usize;
        }
    }
}

async fn reset_store(index: &CodeIndex) {
    index.drop_store().await.ok();
    index.create_store().await.ok();
}

fn bench_batch_composition(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let counter = HuggingFaceCounter::from_embedded().unwrap();

    let index = rt.block_on(async {
        let index = CodeIndex::new(AHNLICH_ADDR, STORE)
            .await
            .expect(
                "Could not connect to Ahnlich. Is `docker compose -f ahnlich-docker-compose.yml up` running?"
            );
        index.create_store().await.ok();
        Arc::new(index)
    });

    // Uniform batch: four chunks, each ~1750 tokens. Sum ~= 7000.
    let uniform_batch: Vec<CodeChunk> = (0..4)
        .map(|i| chunk_with_approx_tokens(&counter, 1750, 50, &format!("uniform_{i}")))
        .collect();

    // Skewed batch: one ~6000-token chunk plus three ~300-token chunks.
    // Sum ~= 6900, deliberately close to the uniform batch's sum.
    let mut skewed_batch = vec![chunk_with_approx_tokens(&counter, 6000, 100, "skewed_big")];
    skewed_batch.extend(
        (0..3).map(|i| chunk_with_approx_tokens(&counter, 300, 20, &format!("skewed_small_{i}"))),
    );

    let uniform_sum: usize = uniform_batch.iter().filter_map(|c| c.token_count).sum();
    let skewed_sum: usize = skewed_batch.iter().filter_map(|c| c.token_count).sum();
    println!("uniform batch token sum: {uniform_sum}");
    println!("skewed batch token sum:  {skewed_sum}");
    println!(
        "skewed batch member tokens: {:?}",
        skewed_batch
            .iter()
            .map(|c| c.token_count)
            .collect::<Vec<_>>()
    );

    let mut group = c.benchmark_group("batch_composition");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    group.bench_function(BenchmarkId::new("composition", "uniform_4x1750"), |b| {
        b.to_async(&rt).iter_custom(|iters| {
            let index = Arc::clone(&index);
            let batch = uniform_batch.clone();
            async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    reset_store(&index).await;
                    let start = std::time::Instant::now();
                    index.add_chunks_batch(&batch).await.unwrap();
                    total += start.elapsed();
                }
                total
            }
        });
    });

    group.bench_function(
        BenchmarkId::new("composition", "skewed_6000_plus_3x300"),
        |b| {
            b.to_async(&rt).iter_custom(|iters| {
                let index = Arc::clone(&index);
                let batch = skewed_batch.clone();
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        reset_store(&index).await;
                        let start = std::time::Instant::now();
                        index.add_chunks_batch(&batch).await.unwrap();
                        total += start.elapsed();
                    }
                    total
                }
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_batch_composition);
criterion_main!(benches);
