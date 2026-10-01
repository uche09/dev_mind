use crate::embeddings::ahnlich::CodeIndex;
use crate::parser::chunk::CodeChunk;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use std::sync::Arc;

pub static TOKEN_THRESHOLD_FOR_SINGLE_PER_BATCH: usize = 1500;

/// Takes a LIST of `CodeChunk` batches and convert it to a STREAM of batches for async processing
/// then sends each group as a single Ahnlich `Set` call (via `CodeIndex::add_chunks_batch`), running
/// at most `concurrency` of those batch calls in flight at the same time.
///
/// `concurrency = 1` is equivalent to "batched, but strictly sequential",
/// useful as a middle data point between the old one-call-per-chunk
/// approach and full concurrent batching.
pub async fn index_batches_bounded(
    index: Arc<CodeIndex>,
    batches: Vec<Vec<CodeChunk>>,
    concurrency: usize,
) -> Vec<(usize, usize, Result<()>)> {
    // let batches: Vec<&[CodeChunk]> = chunks.chunks(batch_size).collect();

    // convert vector of slices into a stream - async equivalent of an iterator
    stream::iter(batches.into_iter().enumerate())
        .map(move |(i, batch)| {
            let index = Arc::clone(&index);
            let batch_len = batch.len();
            async move { (i, batch_len, index.add_chunks_batch(&batch).await) }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await
}

/// Groups `CodeChunk`s into batches based on max number of members.
/// `CodeChunk` with token >= `solo_threshold` ends up alone in its own batch
/// to mitigated excessive padding waste from significantly varying sizes in a batch.
pub fn group_by_token_budget(
    mut chunks: Vec<CodeChunk>,
    solo_threshold: usize,
    max_batch_len: usize,
) -> anyhow::Result<Vec<Vec<CodeChunk>>> {
    // Similarly-sized chunks end up close to each other, minimizing padding waste
    // within any batch this produces.
    chunks.sort_by_key(|c| c.token_count.unwrap_or(usize::MAX));

    let mut batches = Vec::new();
    let mut current = Vec::new();

    for chunk in chunks {
        let tokens = chunk.token_count.unwrap_or(usize::MAX);

        if tokens > solo_threshold {
            // Flush whatever small-chunk batch is in progress,
            // then this chunk goes out completely alone, to mitigate excessive padding waste.
            if !current.is_empty() {
                batches.push(std::mem::take(&mut current));
            }
            batches.push(vec![chunk]);
            continue;
        }

        if current.len() >= max_batch_len {
            batches.push(std::mem::take(&mut current)); // push current saturated batch
        }

        current.push(chunk); // current batch not yet saturated, add new chunk.
    }

    if !current.is_empty() {
        batches.push(current); // push any unsaturated batch from final iteration
    }

    Ok(batches)
}
