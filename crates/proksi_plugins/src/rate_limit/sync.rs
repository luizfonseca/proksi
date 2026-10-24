//! Rate limits shared by replicas through the store

use std::sync::atomic::AtomicBool;

/// Whether rate limits are synced with the store: requests are only counted for it then
pub static SHARING: AtomicBool = AtomicBool::new(false);
