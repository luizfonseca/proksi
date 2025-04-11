use once_cell::sync::OnceCell;
use std::sync::Arc;

use super::adapter::Store;

static GLOBAL_STORE: OnceCell<Arc<dyn Store>> = OnceCell::new();

pub fn init_store<S: Store>(store: S) {
    assert!(
        GLOBAL_STORE.set(Arc::new(store)).is_ok(),
        "Global store already initialized"
    );
}

pub fn get_store() -> &'static Arc<dyn Store> {
    GLOBAL_STORE.get().expect("Global store not initialized")
}
