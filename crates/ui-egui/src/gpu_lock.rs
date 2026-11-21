//! Tests that render a harness run one at a time in each test process.
//!
//! `Harness::render` creates a wgpu device on its first call, and egui_kittest prefers a CPU
//! adapter. When `cargo test` runs several of these tests on parallel threads, the test binary can
//! exit with `0xc0000005` (`STATUS_ACCESS_VIOLATION`). Under cargo-nextest each test is its own
//! process and the lock has no effect.
//!
//! Every test that calls `render()` on a harness takes [`gpu_lock`] on its first line
//! (`let _gpu = crate::gpu_lock::gpu_lock();`) and holds the guard for its whole run.

use std::sync::{Mutex, MutexGuard, PoisonError};

static GPU_LOCK: Mutex<()> = Mutex::new(());

/// Returns a guard that the test holds for its whole run, so tests that render run one at a time.
/// A test that failed while holding it does not block the tests after it.
pub(crate) fn gpu_lock() -> MutexGuard<'static, ()> {
    GPU_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panicked_holder_does_not_block_the_next_test() {
        let _ = std::thread::spawn(|| {
            let _gpu = gpu_lock();
            panic!("a GPU test failed");
        })
        .join();
        drop(gpu_lock());
    }

    #[test]
    fn holders_take_turns() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static INSIDE: AtomicUsize = AtomicUsize::new(0);
        static MAX: AtomicUsize = AtomicUsize::new(0);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    let _gpu = gpu_lock();
                    let now = INSIDE.fetch_add(1, Ordering::SeqCst) + 1;
                    MAX.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    INSIDE.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for t in threads {
            assert!(t.join().is_ok());
        }
        assert_eq!(MAX.load(Ordering::SeqCst), 1);
    }
}
