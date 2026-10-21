//! An allocator counting the allocations of the thread measured, for tests that check a hot path
//! allocates nothing (here, and in proksi).

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
};

use mimalloc::MiMalloc;

thread_local! {
    /// The allocations made on this thread while measured
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The allocator of the tests: mimalloc, counting
pub struct Counting;

fn count() {
    let _ = COUNT.try_with(|count| {
        if let Some(allocations) = count.get() {
            count.set(Some(allocations + 1));
        }
    });
}

// SAFETY: each call goes to mimalloc as it came
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::alloc`'s caller guarantees
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::alloc_zeroed`'s caller guarantees
        unsafe { MiMalloc.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: as `GlobalAlloc::realloc`'s caller guarantees
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as `GlobalAlloc::dealloc`'s caller guarantees
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }
}

/// Asserts that `run` allocates nothing, once warm (lazy statics and caches filled)
///
/// # Panics
///
/// When `run` allocates.
pub fn assert_no_allocations<T>(what: &str, mut run: impl FnMut() -> T) {
    run();
    let (count, _) = allocations(&mut run);
    assert_eq!(count, 0, "{what} allocated {count} times");
}

/// How many allocations `run` makes (on this thread), and what it returns
pub fn allocations<T>(run: impl FnOnce() -> T) -> (usize, T) {
    COUNT.with(|count| count.set(Some(0)));
    let value = run();
    let allocations = COUNT.with(|count| count.replace(None)).unwrap_or_default();
    (allocations, value)
}
