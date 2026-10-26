//! `/api/v1/notifications`: `notify` blocks, from the configuration file or saved in the store
//! (applied by every instance through the store sync). Their secrets are never sent back: URLs
//! are masked, and headers listed by name.

use std::collections::BTreeMap;

use proksi_store::{Expect, Store, Write};
use serde::Serialize;

use super::{
    conflict,
    routes::{self, Refused},
    routes_reply, Reply,
};
use crate::{
    config::notify::{Kind, NotifyBlock},
    notify::stored,
};

/// `PUT` and `DELETE /api/v1/notifications/<name>`
#[derive(Debug, PartialEq, Serialize)]
pub struct Written {
    pub name: String,
    /// The revision of the blocks after the change
    pub revision: u64,
}

/// The blocks, each with where it's from (`file` or `store`), the version it was saved at (for
/// `If-Match`), and its last delivery; and every event they can ask for
pub fn list() -> serde_json::Value {
    let deliveries = crate::notify::deliveries();
    let saved = crate::notify::stored();
    let blocks: Vec<serde_json::Value> = crate::notify::blocks()
        .iter()
        .map(|(name, block)| {
            let version = (!crate::notify::in_file(name))
                .then(|| saved.get(name).map(|stored| stored.version))
                .flatten();
            serde_json::json!({
                "name": name,
                "source": if version.is_some() { "store" } else { "file" },
                "version": version,
                "url": crate::notify::masked(&block.url),
                "format": block.format,
                "on": block.on.iter().map(|kind| kind.name()).collect::<Vec<_>>(),
                "hosts": block.hosts,
                "headers": block.headers.keys().collect::<Vec<_>>(),
                "expiring_days": block.expiring_days,
                "last": deliveries.get(name),
            })
        })
        .collect();
    serde_json::json!({
        "notifications": blocks,
        "events": Kind::ALL.iter().map(|kind| kind.name()).collect::<Vec<_>>(),
    })
}

/// Saves the block `name` to the store. The body is the block as the configuration writes it;
/// a `url`, or a header's value, left out or empty keeps the one saved (the API never sends
/// them back).
pub async fn put(
    store: &dyn Store,
    name: &str,
    body: &[u8],
    write: &Write,
) -> Result<Written, Refused> {
    refuse_file_blocks(name)?;
    check_name(name)?;
    let mut block: serde_json::Value = serde_json::from_slice(body)
        .map_err(|err| Refused::Invalid(format!("invalid JSON: {err}")))?;
    let saved = saved(store, name).await?;
    keep_secrets(&mut block, saved.as_ref()).map_err(Refused::Invalid)?;
    let block: NotifyBlock = serde_json::from_value(block)
        .map_err(|err| Refused::Invalid(format!("invalid block: {err}")))?;
    block.check(name).map_err(Refused::Invalid)?;
    let record = stored::encode(name, &block).map_err(Refused::Invalid)?;
    let revision = store
        .put_notify(record, write)
        .await
        .map_err(routes::refused)?;
    crate::routing::store_source::sync_now();
    Ok(Written {
        name: name.to_string(),
        revision,
    })
}

/// Removes the block `name` from the store (the revision is unchanged if it wasn't there)
pub async fn delete(store: &dyn Store, name: &str, write: &Write) -> Result<Written, Refused> {
    refuse_file_blocks(name)?;
    let revision = store
        .delete_notify(name, write)
        .await
        .map_err(routes::refused)?;
    crate::routing::store_source::sync_now();
    Ok(Written {
        name: name.to_string(),
        revision,
    })
}

/// The answer to a change: a `412` says why, from what the change expected
pub fn reply(result: Result<Written, Refused>, write: &Write) -> Reply {
    let Err(Refused::Conflict { version }) = result else {
        return routes_reply(result);
    };
    let reason = match write.expect {
        Expect::Absent => "a notify block with this name already exists",
        _ if version == 0 => "the notify block isn't in the store anymore",
        _ => "the notify block was changed by someone else",
    };
    conflict(version, reason)
}

/// Blocks of the configuration file are changed there
fn refuse_file_blocks(name: &str) -> Result<(), Refused> {
    if crate::notify::in_file(name) {
        return Err(Refused::Invalid(format!(
            "notify \"{name}\" is in the configuration file: change it there"
        )));
    }
    Ok(())
}

/// Names go in URLs: letters, digits, `-`, `_` and `.`, up to 64
fn check_name(name: &str) -> Result<(), Refused> {
    let valid = (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'));
    if valid {
        return Ok(());
    }
    Err(Refused::Invalid(format!(
        "notify names are letters, digits, `-`, `_` and `.` (up to 64), not {name:?}"
    )))
}

/// The block `name` as saved in the store, if it is
async fn saved(store: &dyn Store, name: &str) -> Result<Option<NotifyBlock>, Refused> {
    let blocks = store.notify_blocks().await.map_err(routes::refused)?;
    let Some(record) = blocks.blocks.iter().find(|record| record.name == name) else {
        return Ok(None);
    };
    stored::decode(record)
        .map(|stored| Some(stored.block))
        .map_err(Refused::Unusable)
}

/// Puts the saved URL and header values back where the change left them out
fn keep_secrets(block: &mut serde_json::Value, saved: Option<&NotifyBlock>) -> Result<(), String> {
    let Some(fields) = block.as_object_mut() else {
        return Err("a notify block is a JSON object".into());
    };
    if fields
        .get("url")
        .and_then(|url| url.as_str())
        .is_none_or(str::is_empty)
    {
        let url = saved
            .map(|saved| saved.url.clone())
            .ok_or("`url` is required")?;
        fields.insert("url".into(), url.into());
    }
    let Some(headers) = fields
        .get_mut("headers")
        .and_then(|headers| headers.as_object_mut())
    else {
        return Ok(());
    };
    let empty = BTreeMap::new();
    let kept = saved.map_or(&empty, |saved| &saved.headers);
    for (name, value) in headers.iter_mut() {
        if value.as_str().is_some_and(str::is_empty) {
            let saved = kept
                .get(name)
                .ok_or(format!("the header {name:?} needs a value"))?;
            *value = saved.clone().into();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proksi_store::{Expect, MemoryStore, Store, Write};

    use super::{delete, put, Refused};
    use crate::notify::stored;

    fn by(expect: Expect) -> Write {
        Write {
            author: Some("ops".into()),
            expect,
        }
    }

    async fn saved(store: &MemoryStore, name: &str) -> crate::config::notify::NotifyBlock {
        let blocks = store.notify_blocks().await.unwrap();
        let record = blocks
            .blocks
            .iter()
            .find(|record| record.name == name)
            .unwrap();
        stored::decode(record).unwrap().block
    }

    #[tokio::test]
    async fn test_blocks_are_saved_keeping_their_secrets() {
        let store = MemoryStore::new();
        let body = br#"{"url":"https://hooks.example.com/secret","format":"slack","headers":{"authorization":"Bearer x"}}"#;
        let written = put(&store, "ops", body, &by(Expect::Absent)).await.unwrap();
        assert_eq!(written.revision, 1);

        // Changed without the URL and the header's value, which the API never sends back
        let body =
            br#"{"url":"","format":"json","on":["upstream_down"],"headers":{"authorization":""}}"#;
        put(&store, "ops", body, &by(Expect::Version(1)))
            .await
            .unwrap();
        let block = saved(&store, "ops").await;
        assert_eq!(block.url, "https://hooks.example.com/secret");
        assert_eq!(block.headers["authorization"], "Bearer x");
        assert_eq!(block.on.len(), 1);

        // A header without a value has none to keep
        let body = br#"{"headers":{"x-new":""}}"#;
        let refused = put(&store, "ops", body, &Write::default())
            .await
            .unwrap_err();
        assert_eq!(
            refused,
            Refused::Invalid("the header \"x-new\" needs a value".into())
        );
    }

    #[tokio::test]
    async fn test_blocks_are_checked() {
        let store = MemoryStore::new();
        let refused = |result: Result<_, Refused>| match result {
            Err(Refused::Invalid(reason)) => reason,
            other => panic!("not refused: {other:?}"),
        };
        assert!(
            refused(put(&store, "ops", br#"{"format":"json"}"#, &Write::default()).await)
                .contains("`url`")
        );
        assert!(
            refused(put(&store, "ops", br#"{"url":"ftp://x"}"#, &Write::default()).await)
                .contains("https://")
        );
        assert!(
            refused(put(&store, "a/b", br#"{"url":"https://x"}"#, &Write::default()).await)
                .contains("names")
        );
        assert!(refused(
            put(
                &store,
                "ops",
                br#"{"url":"https://x","other":1}"#,
                &Write::default()
            )
            .await
        )
        .contains("other"));
        assert!(store.notify_blocks().await.unwrap().blocks.is_empty());
    }

    #[tokio::test]
    async fn test_changes_are_made_from_the_version_read() {
        let store = MemoryStore::new();
        let body = br#"{"url":"https://hooks.example.com/a"}"#;
        put(&store, "ops", body, &by(Expect::Absent)).await.unwrap();
        let again = put(&store, "ops", body, &by(Expect::Absent)).await;
        assert_eq!(again, Err(Refused::Conflict { version: 1 }));
        let stale = delete(&store, "ops", &by(Expect::Version(7))).await;
        assert_eq!(stale, Err(Refused::Conflict { version: 1 }));
        assert_eq!(
            delete(&store, "ops", &by(Expect::Version(1)))
                .await
                .unwrap()
                .revision,
            2
        );
    }
}
