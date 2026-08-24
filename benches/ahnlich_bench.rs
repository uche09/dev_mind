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

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use devmind::embeddings::ahnlich::CodeIndex;
use devmind::indexer::index_batches_bounded;
use devmind::parser::chunk::CodeChunk;
use devmind::parser::parse_file;
use devmind::parser::traverser::{build_ignore_set, collect_rust_files};
use std::sync::Arc;
use std::time::{Duration, Instant};

const AHNLICH_ADDR: &str = "localhost:1370";
const STORE: &str = "devmind_bench";

/// Pulls real CodeChunks out of this project's own source so the benchmark
/// embeds real code, not synthetic strings. Keep `n` modest: every chunk
/// here triggers a real embedding call against a live model, this isn't
/// free CPU-bound work like the parsing benchmarks.
fn collect_sample_chunks(n: usize) -> Vec<CodeChunk> {
    let root = env!("CARGO_MANIFEST_DIR");
    let ignore = build_ignore_set(&["**/target".to_string(), "**/*.toml".to_string()]).unwrap();
    let files = collect_rust_files(root, &ignore).unwrap();

    let mut chunks = Vec::new();
    for file in files {
        chunks.extend(parse_file(&file).unwrap());
        if chunks.len() >= n {
            break;
        }
    }
    chunks.truncate(n);
    chunks
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
        // error_if_exists is false in create_store, so re-running this is safe.
        index.create_store().await.ok();
        Arc::new(index)
    });

    // 40 chunks keeps each benchmark iteration to a reasonable number of
    // real embedding calls. We can raise this later to mimic numbers closer to
    // the 2236-chunk stress test done manually on Ahnlich repo, 
    // but expect each `cargo bench` run to take proportionally longer.
    let sample_size = 40;
    let chunks = collect_sample_chunks(sample_size);
    println!("Benchmarking against {} real chunks from this project", chunks.len());

    let mut group = c.benchmark_group("ahnlich_indexing");
    // Fewer samples than criterion's default (100): each sample here is a
    // real network + inference workload, not a microsecond-scale operation.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    // Baseline: initial implementation, one `Set` call per chunk.
    group.bench_function(BenchmarkId::new("strategy", "sequential_single_calls"), |b| {
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
    });

    // Batching only: same total work, grouped into fewer `Set` calls.
    group.bench_function(BenchmarkId::new("strategy", "batched_no_concurrency"), |b| {
        b.to_async(&rt).iter_custom(|iters| {
            let index = Arc::clone(&index);
            let chunks = chunks.clone();
            async move {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    reset_store(&index).await;

                    let start = Instant::now();
                    for batch in chunks.chunks(16) {
                        index.add_chunks_batch(batch).await.unwrap();
                    }
                    total += start.elapsed();
                }
                total
            }
        });
    });

    // Batching + bounded concurrency: multiple batch calls in flight at once,
    // capped by a semaphore.
    group.bench_function(
        BenchmarkId::new("strategy", "batched_bounded_concurrency_3"),
        |b| {
            b.to_async(&rt).iter_custom(|iters| {
                let index = Arc::clone(&index);
                let chunks = chunks.clone();
                async move {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        reset_store(&index).await;

                        let start = Instant::now();
                        index_batches_bounded(Arc::clone(&index), chunks.clone(), 16, 3)
                            .await
                            .unwrap();
                        total += start.elapsed();
                    }
                    total
                }
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_indexing_strategies);
criterion_main!(benches);
