//! `notify` blocks as the store keeps them: each under its name, as JSON written by Proksi

use crate::route::{Expect, StoreError};

/// A `notify` block: its name, and its settings as written by Proksi
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyRecord {
    pub name: String,
    pub data: String,
    /// Read from the store: the revision it was saved at, which a change can expect
    /// ([`Expect::Version`])
    pub version: u64,
}

impl NotifyRecord {
    /// The record of the block `name`, to write
    #[must_use]
    pub fn new(name: &str, data: String) -> Self {
        Self {
            name: name.to_string(),
            data,
            version: 0,
        }
    }
}

/// All `notify` blocks in the store, at a given revision
#[derive(Debug, Default)]
pub struct NotifySnapshot {
    pub revision: u64,
    pub blocks: Vec<NotifyRecord>,
}

/// Refuses a change to a block saved at `version` (none: not in the store) that `expect`
/// doesn't allow
pub(crate) fn check(expect: Expect, version: Option<u64>) -> Result<(), StoreError> {
    let expected = match expect {
        Expect::Any => true,
        Expect::Absent => version.is_none(),
        Expect::Version(expected) => version.unwrap_or_default() == expected,
    };
    if expected {
        return Ok(());
    }
    Err(StoreError::Conflict {
        version: version.unwrap_or_default(),
    })
}
