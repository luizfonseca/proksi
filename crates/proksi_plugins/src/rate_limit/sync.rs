//! Rate limits shared by replicas through the store: every `SYNC_EVERY`, each block sends the
//! requests its clients made since the last sync, and learns how far ahead each client's
//! arrival time is cluster-wide, raising its own (never lowering it). Requests are decided
//! locally, without waiting for the store; a client can go over by about what it sends in
//! `SYNC_EVERY` on each other replica.

use std::{
    net::IpAddr,
    sync::{
        Arc, LazyLock, Mutex, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use proksi_store::{Rate, Store, StoreError, Used};

use super::Shared;

/// Whether rate limits are synced with the store: requests are only counted for it then
pub static SHARING: AtomicBool = AtomicBool::new(false);

/// How often blocks sync
const SYNC_EVERY: Duration = Duration::from_millis(250);

/// Clients a block counts between two syncs; more are decided locally only until the next
const CLIENTS: usize = 1024;

/// The blocks in use (dropped ones are pruned as blocks are added and synced)
static BLOCKS: LazyLock<Mutex<Vec<Weak<Shared>>>> = LazyLock::new(Mutex::default);

/// Adds a block to the ones synced
pub(crate) fn register(shared: &Arc<Shared>) {
    let mut blocks = BLOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    blocks.retain(|block| block.strong_count() > 0);
    blocks.push(Arc::downgrade(shared));
}

fn live() -> Vec<Arc<Shared>> {
    let mut blocks = BLOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    blocks.retain(|block| block.strong_count() > 0);
    blocks.iter().filter_map(Weak::upgrade).collect()
}

/// Syncs every block once
pub(crate) async fn sync_once(store: &dyn Store) -> Result<(), StoreError> {
    for shared in live() {
        shared.usage.reserve(CLIENTS);
        sync_block(store, &shared).await?;
    }
    Ok(())
}

/// Sends a block's requests, raising its clients to what the store answers. Requests counted
/// when the store can't be reached aren't sent again.
async fn sync_block(store: &dyn Store, shared: &Shared) -> Result<(), StoreError> {
    let used: Vec<(IpAddr, u64)> = shared.usage.take();
    if used.is_empty() {
        return Ok(());
    }
    let rate = Rate {
        interval_us: shared.limits.interval() / 1000,
        capacity_us: shared.limits.capacity() / 1000,
    };
    let counted: Vec<Used> = used
        .iter()
        .map(|(client, requests)| Used {
            client: client.to_string(),
            requests: *requests,
        })
        .collect();
    let ahead = store.sync_rate_limit(&shared.block, rate, &counted).await?;
    let now = shared.limits.now();
    for ((client, _), ahead) in used.iter().zip(ahead) {
        shared
            .limits
            .raise(client, Duration::from_micros(ahead), now);
    }
    Ok(())
}

/// Syncs blocks with `store` for as long as Proksi runs. Without a store that shares rate
/// limits, returns at once, after warning (`warn`) once the first block is in use.
pub async fn run(store: Option<Arc<dyn Store>>, warn: bool) {
    let Some(store) = store else {
        if warn {
            while live().is_empty() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            tracing::warn!(
                "rate_limit blocks count per instance: only the redis store shares them"
            );
        }
        return;
    };
    SHARING.store(true, Ordering::Relaxed);
    let mut interval = tokio::time::interval(SYNC_EVERY);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut failing = false;
    loop {
        interval.tick().await;
        match sync_once(store.as_ref()).await {
            Err(err) if !failing => {
                tracing::warn!("failed to sync rate limits, limiting on this instance only: {err}");
                failing = true;
            }
            Ok(()) if failing => {
                tracing::info!("syncing rate limits again");
                failing = false;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, sync::atomic::Ordering};

    use super::{SHARING, sync_once};
    use crate::{RoutePlugin, rate_limit::RateLimit};

    fn limit(id: &str) -> RateLimit {
        let plugin: RoutePlugin = serde_json::from_value(serde_json::json!({
            "name": "rate_limit",
            "config": { "requests": 2, "per": "1m" },
        }))
        .unwrap();
        RateLimit::new(&plugin, id).unwrap()
    }

    #[tokio::test]
    async fn test_replicas_share_a_limit() {
        let store = proksi_store::MemoryStore::new();
        SHARING.store(true, Ordering::Relaxed);
        // The same block on two replicas: 2 a minute
        let (a, b) = (limit("h / shared"), limit("h / shared"));
        // Room to count, as the sync makes before it's needed
        sync_once(&store).await.unwrap();
        let client: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(a.exceeded(&client).is_none());
        assert!(a.exceeded(&client).is_none());
        // b hasn't heard yet
        assert!(b.exceeded(&client).is_none());
        sync_once(&store).await.unwrap();
        // 3 used of 2 cluster-wide: both refuse it now
        assert!(a.exceeded(&client).is_some());
        assert!(b.exceeded(&client).is_some());
    }

    #[tokio::test]
    async fn test_counting_doesnt_allocate() {
        let store = proksi_store::MemoryStore::new();
        SHARING.store(true, Ordering::Relaxed);
        let limit = limit("h / allocations");
        sync_once(&store).await.unwrap();
        let client: IpAddr = "10.0.0.2".parse().unwrap();
        limit.exceeded(&client);
        crate::allocations::assert_no_allocations("counting a request to sync", || {
            limit.exceeded(&client)
        });
    }
}
