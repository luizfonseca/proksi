//! Rate limits shared by replicas through the store: every `SYNC_EVERY`, each block moves its
//! clients' cursors in the store (a cursor per client, in a namespace per block) by what their
//! requests since the last sync cost, and learns how far ahead each client's arrival time is
//! cluster-wide, raising its own (never lowering it). Requests are decided locally, without
//! waiting for the store; a client can go over by about what it sends in `SYNC_EVERY` on each
//! other replica.

use std::{
    net::IpAddr,
    sync::{
        Arc, LazyLock, Mutex, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use proksi_store::{Store, StoreError};

use super::Shared;

/// Whether rate limits are synced with the store: requests are only counted for it then
pub static SHARING: AtomicBool = AtomicBool::new(false);

/// How often blocks sync
const SYNC_EVERY: Duration = Duration::from_millis(250);

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

/// Syncs every block once, saying whether the store was asked anything (with nothing to send,
/// it can't tell whether the store is up)
pub(crate) async fn sync_once(store: &dyn Store) -> Result<bool, StoreError> {
    let mut asked = false;
    for shared in live() {
        shared.usage.make_room();
        asked |= sync_block(store, &shared).await?;
    }
    Ok(asked)
}

/// Sends a block's requests, raising its clients to what the store answers, and says whether
/// there was anything to send. Requests counted when the store can't be reached aren't sent
/// again.
async fn sync_block(store: &dyn Store, shared: &Shared) -> Result<bool, StoreError> {
    let used: Vec<(IpAddr, u64)> = shared.usage.take();
    if used.is_empty() {
        return Ok(false);
    }
    let steps = steps(&used, shared.limits.interval());
    let ahead = store.advance_cursors(&shared.block, &steps).await?;
    let now = shared.limits.now();
    for ((client, _), ahead) in used.iter().zip(ahead) {
        shared
            .limits
            .raise(client, Duration::from_micros(ahead), now);
    }
    Ok(true)
}

/// How far each client's cursor moves: its requests times `interval` (in nanoseconds), in
/// microseconds rounded up, so a request never counts for less than it costs
fn steps(used: &[(IpAddr, u64)], interval: u64) -> Vec<(String, u64)> {
    let interval_us = interval.div_ceil(1000).max(1);
    used.iter()
        .map(|(client, requests)| (client.to_string(), requests.saturating_mul(interval_us)))
        .collect()
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
            Ok(true) if failing => {
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

    use super::{SHARING, steps, sync_block, sync_once};
    use crate::{RoutePlugin, rate_limit::RateLimit};

    /// `sync_once` syncs every block: tests syncing take turns, so none takes another's counts
    static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn limit(id: &str) -> RateLimit {
        let plugin: RoutePlugin = serde_json::from_value(serde_json::json!({
            "name": "rate_limit",
            "config": { "requests": 2, "per": "1m" },
        }))
        .unwrap();
        RateLimit::new(&plugin, id).unwrap()
    }

    /// A block that saw a request, so the sync made room to count its clients
    async fn used(store: &dyn proksi_store::Store, id: &str) -> RateLimit {
        let limit = limit(id);
        limit.exceeded(&"10.9.9.9".parse().unwrap());
        sync_once(store).await.unwrap();
        limit
    }

    #[test]
    fn test_steps_round_up_to_whole_microseconds() {
        let client: IpAddr = "10.0.0.4".parse().unwrap();
        // 1.5µs between requests: 3 requests move the client 6µs (never less than they cost)
        assert_eq!(
            steps(&[(client, 3)], 1_500),
            vec![("10.0.0.4".to_string(), 6)]
        );
        // Under 1µs between requests: 1µs each
        assert_eq!(
            steps(&[(client, 5)], 300),
            vec![("10.0.0.4".to_string(), 5)]
        );
    }

    #[tokio::test]
    async fn test_replicas_share_a_limit() {
        let _turn = SYNCING.lock().await;
        let store = proksi_store::MemoryStore::new();
        SHARING.store(true, Ordering::Relaxed);
        // The same block on two replicas: 2 a minute
        let (a, b) = (
            used(&store, "h / shared").await,
            used(&store, "h / shared").await,
        );
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
    async fn test_syncing_says_whether_it_reached_the_store() {
        let _turn = SYNCING.lock().await;
        let store = proksi_store::MemoryStore::new();
        SHARING.store(true, Ordering::Relaxed);
        let limit = used(&store, "h / reached").await;
        let client: IpAddr = "10.0.0.3".parse().unwrap();
        limit.exceeded(&client);
        assert_eq!(sync_block(&store, &limit.shared).await, Ok(true));
        // Nothing to send: the store wasn't asked, so it can't tell it's up
        assert_eq!(sync_block(&store, &limit.shared).await, Ok(false));
    }

    #[tokio::test]
    async fn test_counting_doesnt_allocate() {
        let _turn = SYNCING.lock().await;
        let store = proksi_store::MemoryStore::new();
        SHARING.store(true, Ordering::Relaxed);
        let limit = used(&store, "h / allocations").await;
        let client: IpAddr = "10.0.0.2".parse().unwrap();
        limit.exceeded(&client);
        crate::allocations::assert_no_allocations("counting a request to sync", || {
            limit.exceeded(&client)
        });
    }
}
