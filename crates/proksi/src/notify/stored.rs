//! `notify` blocks as Proksi writes them to the store: `{"schema_version": 1, "block": {...}}`,
//! the block as the configuration has it

use proksi_store::NotifyRecord;
use serde::{Deserialize, Serialize};

use super::Stored;
use crate::config::notify::NotifyBlock;

/// The version of the format, for when it changes
const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Saved {
    schema_version: u32,
    block: NotifyBlock,
}

/// The record of the block `name`, to write
pub fn encode(name: &str, block: &NotifyBlock) -> Result<NotifyRecord, String> {
    let saved = Saved {
        schema_version: SCHEMA_VERSION,
        block: block.clone(),
    };
    let data = serde_json::to_string(&saved).map_err(|err| err.to_string())?;
    Ok(NotifyRecord::new(name, data))
}

/// The block a record holds, if it can be used
pub fn decode(record: &NotifyRecord) -> Result<Stored, String> {
    let saved: Saved = serde_json::from_str(&record.data).map_err(|err| err.to_string())?;
    if saved.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema version {}",
            saved.schema_version
        ));
    }
    saved.block.check(&record.name)?;
    Ok(Stored {
        block: saved.block,
        version: record.version,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proksi_store::NotifyRecord;

    use super::{decode, encode};
    use crate::config::notify::{Format, Kind, NotifyBlock};

    #[test]
    fn test_blocks_are_read_as_written() {
        let block = NotifyBlock {
            url: "https://hooks.example.com/a".into(),
            format: Format::Slack,
            on: vec![Kind::UpstreamDown],
            hosts: vec!["*.example.com".into()],
            headers: BTreeMap::from([("authorization".to_string(), "Bearer x".to_string())]),
            expiring_days: 7,
        };
        let mut record = encode("ops", &block).unwrap();
        record.version = 4;
        let stored = decode(&record).unwrap();
        assert_eq!((stored.block, stored.version), (block, 4));

        // Blocks that can't be used aren't
        let invalid = NotifyRecord::new(
            "ops",
            r#"{"schema_version":1,"block":{"url":"ftp://x"}}"#.into(),
        );
        assert!(decode(&invalid).unwrap_err().contains("https://"));
        let newer = NotifyRecord::new(
            "ops",
            r#"{"schema_version":2,"block":{"url":"https://x"}}"#.into(),
        );
        assert!(decode(&newer).unwrap_err().contains("schema version 2"));
    }
}
