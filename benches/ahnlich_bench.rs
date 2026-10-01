// Requires a running Ahnlich instance:
//     docker compose -f ahnlich-docker-compose.yml up
// This is an integration benchmark, not a hermetic one: it makes real gRPC
// calls to a real embedding model, so results depend on your machine and
// whatever else is running on it.
//
// Uses a separate store name ("devmind_bench") so results never mix with
// the real project's indexed data.
//
// Run with: cargo bench --bench ahnlich_bench

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use devmind::embeddings::ahnlich::CodeIndex;
use devmind::indexer::{
    TOKEN_THRESHOLD_FOR_SINGLE_PER_BATCH, group_by_token_budget, index_batches_bounded,
};
use devmind::parser::chunk::{ChunkTokenBound, CodeChunk};
use devmind::parser::parse_file;
use devmind::parser::tokenizer::{ChunkSplitter, HuggingFaceCounter};
use devmind::parser::traverser::{build_ignore_set, collect_rust_files};
use std::sync::Arc;
use std::time::{Duration, Instant};

const AHNLICH_ADDR: &str = "localhost:1370";
const STORE: &str = "devmind_bench";
const MAX_BATCH_LEN: usize = 4;

/// Pulls real CodeChunks out of this project's own source so the benchmark
/// embeds real code, not synthetic strings. Keep `n` modest: every chunk
/// here triggers a real embedding call against a live model, this isn't
/// free CPU-bound work like the parsing benchmarks.
fn collect_sample_chunks(n: usize, splitter: &ChunkSplitter) -> anyhow::Result<Vec<CodeChunk>> {
    let root = env!("CARGO_MANIFEST_DIR");
    let ignore = build_ignore_set(&["**/target".to_string(), "**/*.toml".to_string()])?;
    let files = collect_rust_files(root, &ignore)?;

    let mut raw_chunks = Vec::new();
    for file in files {
        raw_chunks.extend(parse_file(&file)?);
        if raw_chunks.len() >= n {
            break;
        }
    }
    raw_chunks.truncate(n);

    let mut bounded_chunks = Vec::new();
    for chunk in raw_chunks {
        bounded_chunks.extend(splitter.split(chunk)?);
    }

    Ok(bounded_chunks)
}

/// Drops and recreates the store, ignoring errors from either call (drop
/// fails harmlessly if the store doesn't exist yet, create is idempotent
/// via error_if_exists: false). This is what gives every single sample a
/// clean, empty store to measure against, instead of one that's still
/// carrying entries from whichever benchmark happened to run before it.
async fn reset_store(index: &CodeIndex) {
    index.drop_store().await.ok();
    index.create_store().await.ok();
}

fn bench_indexing_strategies(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let index = rt.block_on(async {
        let index = CodeIndex::new(AHNLICH_ADDR, STORE)
            .await
            .expect(
                "Could not connect to Ahnlich. Is `docker compose -f ahnlich-docker-compose.yml up` running?"
            );
        index.create_store().await.ok();
        Arc::new(index)
    });

    // Real tokenizer + real bound, matching main.rs exactly, so the
    // benchmark's batch shapes match what a real `devmind index` run
    // would actually produce.
    let token_counter = HuggingFaceCounter::from_embedded().unwrap();
    let splitter = ChunkSplitter::new(&token_counter, ChunkTokenBound::default());

    // 40 chunks keeps each benchmark iteration to a reasonable number of
    // real embedding calls. We can raise this later to mimic numbers closer to
    // the 2236-chunk stress test done manually on the Ahnlich repo,
    // but expect each `cargo bench` run to take proportionally longer.
    //
    // Note: this project's own source is unlikely to contain the kind of
    // large, single-item chunks (generated service files, big model
    // config structs) that triggered the real crashes against the
    // Ahnlich repo. This benchmark exercises the common case well; it
    // does not stress the split-heavy path. If you want confidence there
    // too, point `collect_rust_files` at a heavier codebase (e.g. a
    // checked-out copy of ahnlich itself) for a separate run.
    let sample_size = 40;
    let chunks = collect_sample_chunks(sample_size, &splitter).unwrap();
    println!(
        "Benchmarking against {} bounded chunks from this project",
        chunks.len()
    );

    let mut group = c.benchmark_group("ahnlich_indexing");
    // Fewer samples than criterion's default (100): each sample here is a
    // real network + inference workload, not a microsecond-scale operation.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    // Baseline: one `Set` call per chunk, no batching, no grouping.
    // Still the right reference point: everything else should be judged
    // against "what if we did the simplest possible thing."
    group.bench_function(
        BenchmarkId::new("strategy", "sequential_single_calls"),
        |b| {
            b.to_async(&rt).iter_custom(|iters| {
                let index = Arc::clone(&index);
                let chunks = chunks.clone();
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        reset_store(&index).await; // setup: not timed

                        let start = Instant::now(); // routine: timed
                        for chunk in &chunks {
                            index.add_chuck(chunk).await.unwrap();
                        }
                        total += start.elapsed();
                    }
                    total
                }
            });
        },
    );

    // Token-budgeted batching (the current real strategy), swept across
    // concurrency = 1, 2, 3. Grouping itself (group_by_token_budget) is
    // cheap, pure CPU work, so it's fine to redo it inside the timed
    // region per iteration; it's not what we're measuring.
    for concurrency in [1usize, 2, 3] {
        group.bench_function(
            BenchmarkId::new(
                "strategy",
                format!("token_budgeted_concurrency_{concurrency}"),
            ),
            |b| {
                b.to_async(&rt).iter_custom(|iters| {
                    let index = Arc::clone(&index);
                    let chunks = chunks.clone();
                    async move {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            reset_store(&index).await;

                            let batches = group_by_token_budget(
                                chunks.clone(),
                                TOKEN_THRESHOLD_FOR_SINGLE_PER_BATCH,
                                MAX_BATCH_LEN,
                            )
                            .unwrap();

                            let start = Instant::now();
                            index_batches_bounded(Arc::clone(&index), batches, concurrency).await;
                            total += start.elapsed();
                        }
                        total
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_indexing_strategies);
criterion_main!(benches);
