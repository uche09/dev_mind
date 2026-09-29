use crate::{embeddings::ahnlich::CodeIndex};
use crate::parser::chunk::CodeChunk;
use anyhow::Result;
use futures::stream::{self, StreamExt};
use std::sync::Arc;

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
            async move {
                (i, batch_len, index.add_chunks_batch(&batch).await)
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await
}

/// Groups `CodeChunk`s into batches based on batched token padding cost logic and/or max number of members
pub fn group_by_token_budget(
    mut chunks: Vec<CodeChunk>,
    max_batch_padding_cost: usize,
    max_batch_len: usize,
) -> anyhow::Result<Vec<Vec<CodeChunk>>> {
    // Similarly-sized chunks end up close to each other, minimizing padding waste
    // within any batch this produces.
    chunks.sort_by_key(|c| c.token_count.unwrap_or(usize::MAX));

    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_max = 0usize;

    for chunk in chunks {
        let tokens = chunk.token_count.unwrap_or(usize::MAX);
        let projected_max = current_max.max(tokens); // token of the longest member
        
        // Performance pattern inidicates that every member of the batch gets **padded**
        // to the longest member in the batch, which is the actual batch cost,
        // not what the memebers individually add up to.
        let projected_cost = (current.len() + 1) * projected_max;

        let exceeds_cost_budget = projected_cost > max_batch_padding_cost;
        let exceeds_max_amnt_members = current.len() >= max_batch_len;

        if !current.is_empty() && (exceeds_cost_budget || exceeds_max_amnt_members)  {
            batches.push(std::mem::take(&mut current)); // push current saturated batch
            current_max = 0; // reset to start new batch
        }

        current_max += current_max.max(tokens);
        current.push(chunk); // current batch not yet saturated, add new chunk.
    }

    if !current.is_empty() {
        batches.push(current); // push any unsaturated batch from final iteration
    }

    Ok(batches)
}