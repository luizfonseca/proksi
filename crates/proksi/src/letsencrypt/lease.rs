//! Ordering each certificate once, however many replicas share the store (ADR 0002): the
//! replica holding a domain's lease orders it, the others skip it

use std::{future::Future, time::Duration};

use proksi_store::Store;

/// How long a lease is held at most, if its holder dies before releasing it
pub const LEASE_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The work ran and succeeded
    Done,
    /// Another replica holds the lease
    Held,
    /// No longer needed once the lease was taken (another replica just did it)
    NotNeeded,
    /// The work failed (the lease was released)
    Failed(String),
    /// The lease couldn't be taken (the store is unavailable)
    Unavailable(String),
}

/// Runs `work` holding the lease `name`, if `needed` still says so once it is taken.
/// The lease is released afterwards, also when `work` fails.
pub async fn with_lease<N, W>(
    store: &dyn Store,
    holder: &str,
    name: &str,
    ttl: Duration,
    needed: N,
    work: W,
) -> Outcome
where
    N: Future<Output = bool>,
    W: Future<Output = Result<(), anyhow::Error>>,
{
    match store.try_lock(name, holder, ttl).await {
        Ok(true) => {}
        Ok(false) => return Outcome::Held,
        Err(err) => return Outcome::Unavailable(err.to_string()),
    }

    let outcome = if needed.await {
        match work.await {
            Ok(()) => Outcome::Done,
            Err(err) => Outcome::Failed(err.to_string()),
        }
    } else {
        Outcome::NotNeeded
    };

    // Otherwise it expires after `ttl`
    if let Err(err) = store.unlock(name, holder).await {
        tracing::warn!("failed to release the lease {name}: {err}");
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use proksi_store::{test_util::certificate, MemoryStore, Store};

    use super::{with_lease, Outcome};

    const MINUTE: Duration = Duration::from_secs(60);

    /// A replica ordering `domain`'s certificate if the store has none, counting orders
    async fn order(
        store: &dyn Store,
        replica: &str,
        domain: &str,
        orders: &AtomicUsize,
    ) -> Outcome {
        with_lease(
            store,
            replica,
            &format!("acme:{domain}"),
            MINUTE,
            async { store.get_certificate(domain).await.is_none() },
            async {
                orders.fetch_add(1, Ordering::SeqCst);
                // Ordering takes a while
                tokio::time::sleep(Duration::from_millis(100)).await;
                store
                    .set_certificate(domain, certificate(domain, 90))
                    .await
                    .map_err(|err| anyhow::anyhow!("{err}"))
            },
        )
        .await
    }

    #[tokio::test]
    async fn test_replicas_order_a_domain_once() {
        let store = Arc::new(MemoryStore::new());
        let orders = AtomicUsize::new(0);

        let (a, b) = tokio::join!(
            order(store.as_ref(), "r1", "a.test", &orders),
            order(store.as_ref(), "r2", "a.test", &orders),
        );

        assert_eq!(orders.load(Ordering::SeqCst), 1);
        assert_eq!((a, b), (Outcome::Done, Outcome::Held));
        // Later checks find the certificate
        assert_eq!(
            order(store.as_ref(), "r2", "a.test", &orders).await,
            Outcome::NotNeeded
        );
        assert_eq!(orders.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "store-sqlite")]
    #[tokio::test]
    async fn test_replicas_on_sqlite_order_a_domain_once() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("proksi.db").to_string_lossy()
        );
        let replica_a = proksi_store::SqlStore::connect(&url).unwrap();
        let replica_b = proksi_store::SqlStore::connect(&url).unwrap();
        let orders = AtomicUsize::new(0);

        let (a, b) = tokio::join!(
            order(&replica_a, "r1", "a.test", &orders),
            order(&replica_b, "r2", "a.test", &orders),
        );

        assert_eq!(orders.load(Ordering::SeqCst), 1);
        assert!(matches!(
            (&a, &b),
            (Outcome::Done, Outcome::Held) | (Outcome::Held, Outcome::Done)
        ));
    }

    #[tokio::test]
    async fn test_a_failed_order_releases_the_lease() {
        let store = MemoryStore::new();

        let outcome = with_lease(&store, "r1", "acme:a.test", MINUTE, async { true }, async {
            Err(anyhow::anyhow!("rate limited"))
        })
        .await;

        assert_eq!(outcome, Outcome::Failed("rate limited".to_string()));
        assert!(store.try_lock("acme:a.test", "r2", MINUTE).await.unwrap());
    }

    #[tokio::test]
    async fn test_an_expired_lease_is_picked_up() {
        let store = MemoryStore::new();
        let orders = AtomicUsize::new(0);
        // A replica took the lease, then died
        assert!(store
            .try_lock("acme:a.test", "r1", Duration::from_millis(200))
            .await
            .unwrap());
        assert_eq!(order(&store, "r2", "a.test", &orders).await, Outcome::Held);

        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(order(&store, "r2", "a.test", &orders).await, Outcome::Done);
        assert_eq!(orders.load(Ordering::SeqCst), 1);
    }
}
