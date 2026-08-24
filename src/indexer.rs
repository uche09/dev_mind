use crate::embeddings::ahnlich::CodeIndex;
use crate::parser::chunk::CodeChunk;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Semaphore;


/// Splits `chunks` into groups of `batch_size` and sends each group as a
/// single Ahnlich `Set` call (via `CodeIndex::add_chunks_batch`), running
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
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));

    let batches: Vec<Vec<CodeChunk>> = chunks
        .chunks(batch_size.max(1))
        .map(|slice| slice.to_vec())
        .collect();

    let mut handles = Vec::with_capacity(batches.len());

    for batch in batches {
        // Blocks here if `concurrency` batches are already in flight.
        // This is the backpressure mechanism: the loop can't outrun the
        // limit no matter how fast it tries to spawn new tasks.
        let permit = semaphore.clone().acquire_owned().await?;
        let index = Arc::clone(&index);

        let handle = tokio::spawn(async move {
            let result = index.add_chunks_batch(&batch).await;
            drop(permit); // release the slot back to the semaphore
            result
        });
        handles.push(handle);
    }

    // Wait for every spawned task to finish, and surface the first error
    // (either a JoinError from the task itself, or an error the task
    // returned from add_chunks_batch).
    for handle in handles {
        handle.await??;
    }

    Ok(())
}
