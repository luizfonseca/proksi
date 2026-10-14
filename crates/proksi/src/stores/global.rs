use once_cell::sync::OnceCell;
use std::sync::Arc;

use super::{store_trait::Store, MemoryStore};

static GLOBAL_STORE: OnceCell<Arc<dyn Store>> = OnceCell::new();

pub fn init_store<S: Store>(store: S) {
    if GLOBAL_STORE.set(Arc::new(store)).is_err() {
        tracing::error!("failed to initialize global store");
    }
}

/// Returns the global store, falling back to a memory store if it was not initialized
pub fn get_store() -> &'static Arc<dyn Store> {
    GLOBAL_STORE.get_or_init(|| {
        tracing::warn!("global store not initialized, using memory store");
        Arc::new(MemoryStore::new())
    })
}
