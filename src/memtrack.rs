// # AI GENERATED
//
// A minimal heap-memory tracker built by wrapping Rust's
// `GlobalAlloc` trait, the same trait `System` (the default allocator)
// implements. Every single heap allocation and deallocation in the whole
// process, std collections, String, tokio's internals, the Ahnlich client,
// everything, routes through whatever type is registered with
// `#[global_allocator]`. Wrapping it means we intercept every `alloc` and
// `dealloc` call site for free, without touching any other code.
//
// Only compiled in behind the `mem_profile` feature so normal builds pay
// zero cost (not even the atomic increments).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct TrackingAllocator;

// Two counters:
//   CURRENT — bytes currently live on the heap right now.
//   PEAK    — the highest CURRENT has ever reached.
// AtomicUsize (not a plain usize behind a Mutex) matters here because
// allocations can happen from multiple OS threads at once, e.g. once you
// add bounded concurrency with tokio::spawn, each spawned task may run on
// a different worker thread, all allocating concurrently. A plain counter
// would need locking around every alloc/dealloc, which would slow down
// every allocation in the program. Atomics give thread-safe increments
// without a lock.
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

// This is the line that actually activates the tracker: it tells the Rust
// compiler "route every allocation in this binary through
// TrackingAllocator instead of the default System allocator." Only one
// #[global_allocator] item may exist in a compiled binary, which is why
// this whole file is feature-gated instead of always-on.
#[cfg(feature = "mem_profile")]
#[global_allocator]
static GLOBAL: TrackingAllocator = TrackingAllocator;

/// Bytes currently live on the heap right now.
pub fn current_bytes() -> usize {
    CURRENT.load(Ordering::Relaxed)
}

/// The highest `current_bytes()` has reached since the last `reset_peak()`.
pub fn peak_bytes() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Re-baselines the peak counter to the current allocation level.
/// Call this right before the section of code you want to measure, so
/// earlier setup (arg parsing, connecting to Ahnlich) doesn't count
/// toward the number you actually care about.
pub fn reset_peak() {
    PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn human_bytes(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{:.2} {}", size, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_reasonably() {
        assert_eq!(human_bytes(512), "512.00 B");
        assert_eq!(human_bytes(2048), "2.00 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.00 MB");
    }
}
