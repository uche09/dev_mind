use crate::embeddings::ahnlich::CodeIndex;
use crate::parser::chunk::CodeChunk;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::time::{Duration, sleep};

/// Splits `chunks` into groups of `batch_size` and convert it to a stream of batches
/// then sends each group as a single Ahnlich `Set` call (via `CodeIndex::add_chunks_batch`), running
/// at most `concurrency` of those batch calls in flight at the same time.
///
/// `concurrency = 1` is equivalent to "batched, but strictly sequential",
/// useful as a middle data point between the old one-call-per-chunk
/// approach and full concurrent batching.
pub async fn index_batches_bounded(
    index: Arc<CodeIndex>,
    chunks: Vec<CodeChunk>,
    batch_size: usize,
    concurrency: usize,
) -> Vec<(usize, usize, Result<()>)> {
    let batches: Vec<&[CodeChunk]> = chunks.chunks(batch_size).collect();

    // convert vector of slices into a stream - async equivalent of an iterator
    stream::iter(batches.into_iter().enumerate())
        .map(move |(i, batch)| {
            let index = Arc::clone(&index);
            let batch_len = batch.len();
            async move {
                sleep(Duration::from_millis(500)).await;
                (i, batch_len, index.add_chunks_batch(batch).await)
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await
}
