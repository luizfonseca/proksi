use once_cell::sync::OnceCell;
use std::sync::Arc;

use super::store_trait::Store;

static GLOBAL_STORE: OnceCell<Arc<dyn Store>> = OnceCell::new();

pub fn init_store<S: Store>(store: S) {
    if GLOBAL_STORE.set(Arc::new(store)).is_err() {
        tracing::error!("failed to initialize global store");
    }
}

pub fn get_store() -> &'static Arc<dyn Store> {
    GLOBAL_STORE.get().expect("Global store not initialized")
}
