//! Versions and history of hosts in SQL (migration `0004_route_history.sql`).
//!
//! `proksi_routes` keeps the active definition of each host, read by every replica (and by
//! older Proksi versions, which know nothing else). `proksi_route_versions` keeps the versions,
//! `proksi_route_changes` the changes, and `proksi_route_state` the active version and last
//! change of each host.
//!
//! A change first increments the store's revision: the row stays locked until the change
//! commits, so changes run one at a time and what one expects can't change under it.

use sqlx::{AnyConnection, AnyPool, Row};

use crate::route::{
    Change, Expect, HistoryEntry, RouteRecord, RouteVersion, StoreError, Write,
};

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn invalid(err: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(err.to_string())
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or_default()
}

fn to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(invalid)
}

/// Increments the store's revision, locking it until the transaction ends
async fn next_revision(tx: &mut AnyConnection) -> Result<u64, StoreError> {
    let row = sqlx::query(
        "UPDATE proksi_meta SET value = value + 1 WHERE key = 'revision' RETURNING value",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(unavailable)?;
    to_u64(row.try_get("value").map_err(invalid)?)
}

/// The host's last change (0 if none) and whether it's in the store
async fn state(tx: &mut AnyConnection, host: &str) -> Result<(u64, bool), StoreError> {
    let changed = sqlx::query("SELECT changed FROM proksi_route_state WHERE host = $1")
        .bind(host)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?
        .map(|row| row.try_get::<i64, _>("changed"))
        .transpose()
        .map_err(invalid)?
        .unwrap_or_default();
    let present = sqlx::query("SELECT 1 FROM proksi_routes WHERE host = $1")
        .bind(host)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?
        .is_some();
    Ok((to_u64(changed)?, present))
}

/// Refuses a change the host doesn't expect. Returns whether the host is in the store.
async fn check(tx: &mut AnyConnection, host: &str, expect: Expect) -> Result<bool, StoreError> {
    let (changed, present) = state(tx, host).await?;
    let expected = match expect {
        Expect::Any => true,
        Expect::Absent => !present,
        Expect::Changed(expected) => expected == changed,
    };
    if !expected {
        return Err(StoreError::Conflict { changed, present });
    }
    Ok(present)
}

/// Writes the active definition (read by replicas) and the host's state
async fn activate(
    tx: &mut AnyConnection,
    host: &str,
    route: Option<(i64, &str)>,
    version: u64,
    revision: u64,
) -> Result<(), StoreError> {
    match route {
        Some((schema_version, data)) => sqlx::query(
            "INSERT INTO proksi_routes (host, schema_version, data, updated_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (host) DO UPDATE SET schema_version = excluded.schema_version, \
             data = excluded.data, updated_at = excluded.updated_at",
        )
        .bind(host)
        .bind(schema_version)
        .bind(data)
        .bind(unix_timestamp())
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?,
        None => sqlx::query("DELETE FROM proksi_routes WHERE host = $1")
            .bind(host)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?,
    };
    sqlx::query(
        "INSERT INTO proksi_route_state (host, version, changed) VALUES ($1, $2, $3) \
         ON CONFLICT (host) DO UPDATE SET version = excluded.version, changed = excluded.changed",
    )
    .bind(host)
    .bind(to_i64(version)?)
    .bind(to_i64(revision)?)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// Records a change at `revision`, and keeps the last `keep` changes and versions (and the
/// active version)
async fn record(
    tx: &mut AnyConnection,
    host: &str,
    revision: u64,
    change: Change,
    write: &Write,
    keep: usize,
) -> Result<(), StoreError> {
    let (kind, target) = match change {
        Change::Saved => ("saved", None),
        Change::Deleted => ("deleted", None),
        Change::RolledBack { to } => ("rolled_back", Some(to_i64(to)?)),
    };
    sqlx::query(
        "INSERT INTO proksi_route_changes (host, revision, created_at, author, kind, target) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(host)
    .bind(to_i64(revision)?)
    .bind(unix_timestamp())
    .bind(write.author.clone())
    .bind(kind)
    .bind(target)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;

    let keep = i64::try_from(keep).map_err(invalid)?;
    sqlx::query(
        "DELETE FROM proksi_route_changes WHERE host = $1 AND revision NOT IN \
         (SELECT revision FROM proksi_route_changes WHERE host = $1 ORDER BY revision DESC LIMIT $2)",
    )
    .bind(host)
    .bind(keep)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    trim_versions(tx, host, keep).await
}

/// Removes the oldest versions but the active one, until `keep` are left
async fn trim_versions(tx: &mut AnyConnection, host: &str, keep: i64) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT v.revision, s.version FROM proksi_route_versions v \
         JOIN proksi_route_state s ON s.host = v.host WHERE v.host = $1 ORDER BY v.revision",
    )
    .bind(host)
    .fetch_all(&mut *tx)
    .await
    .map_err(unavailable)?;
    let excess = i64::try_from(rows.len()).map_err(invalid)? - keep;
    let mut removable = vec![];
    for row in &rows {
        let revision: i64 = row.try_get("revision").map_err(invalid)?;
        let active: i64 = row.try_get("version").map_err(invalid)?;
        if revision != active {
            removable.push(revision);
        }
    }
    for revision in removable.into_iter().take(usize::try_from(excess).unwrap_or_default()) {
        sqlx::query("DELETE FROM proksi_route_versions WHERE host = $1 AND revision = $2")
            .bind(host)
            .bind(revision)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
    }
    Ok(())
}

pub(crate) async fn put(
    pool: AnyPool,
    route: RouteRecord,
    write: Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = route.host.to_lowercase();
    let schema_version = i64::from(route.schema_version);
    let mut tx = pool.begin().await.map_err(unavailable)?;
    let revision = next_revision(&mut tx).await?;
    check(&mut tx, &host, write.expect).await?;
    sqlx::query(
        "INSERT INTO proksi_route_versions (host, revision, schema_version, data, created_at, author) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&host)
    .bind(to_i64(revision)?)
    .bind(schema_version)
    .bind(&route.data)
    .bind(unix_timestamp())
    .bind(write.author.clone())
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    activate(&mut tx, &host, Some((schema_version, &route.data)), revision, revision).await?;
    record(&mut tx, &host, revision, Change::Saved, &write, keep).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok(revision)
}

pub(crate) async fn delete(
    pool: AnyPool,
    host: String,
    write: Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = host.to_lowercase();
    let mut tx = pool.begin().await.map_err(unavailable)?;
    let revision = next_revision(&mut tx).await?;
    if !check(&mut tx, &host, write.expect).await? {
        // Nothing to delete: the revision is unchanged (the transaction is rolled back)
        return Ok(revision - 1);
    }
    activate(&mut tx, &host, None, 0, revision).await?;
    record(&mut tx, &host, revision, Change::Deleted, &write, keep).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok(revision)
}

pub(crate) async fn rollback(
    pool: AnyPool,
    host: String,
    to: u64,
    write: Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = host.to_lowercase();
    let mut tx = pool.begin().await.map_err(unavailable)?;
    let revision = next_revision(&mut tx).await?;
    let version = sqlx::query(
        "SELECT schema_version, data FROM proksi_route_versions WHERE host = $1 AND revision = $2",
    )
    .bind(&host)
    .bind(to_i64(to)?)
    .fetch_optional(&mut *tx)
    .await
    .map_err(unavailable)?
    .ok_or(StoreError::NoVersion(to))?;
    check(&mut tx, &host, write.expect).await?;
    let schema_version: i64 = version.try_get("schema_version").map_err(invalid)?;
    let data: String = version.try_get("data").map_err(invalid)?;
    activate(&mut tx, &host, Some((schema_version, &data)), to, revision).await?;
    record(&mut tx, &host, revision, Change::RolledBack { to }, &write, keep).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok(revision)
}

pub(crate) async fn history(pool: AnyPool, host: String) -> Result<Vec<HistoryEntry>, StoreError> {
    let rows = sqlx::query(
        "SELECT revision, created_at, author, kind, target FROM proksi_route_changes \
         WHERE host = $1 ORDER BY revision DESC",
    )
    .bind(host.to_lowercase())
    .fetch_all(&pool)
    .await
    .map_err(unavailable)?;
    rows.iter().map(entry).collect()
}

fn entry(row: &sqlx::any::AnyRow) -> Result<HistoryEntry, StoreError> {
    let kind: String = row.try_get("kind").map_err(invalid)?;
    let change = match kind.as_str() {
        "saved" => Change::Saved,
        "deleted" => Change::Deleted,
        "rolled_back" => Change::RolledBack {
            to: to_u64(row.try_get::<Option<i64>, _>("target").map_err(invalid)?.unwrap_or_default())?,
        },
        other => return Err(StoreError::Invalid(format!("unknown change {other:?}"))),
    };
    Ok(HistoryEntry {
        revision: to_u64(row.try_get("revision").map_err(invalid)?)?,
        at: to_u64(row.try_get("created_at").map_err(invalid)?)?,
        author: row.try_get("author").map_err(invalid)?,
        change,
    })
}

pub(crate) async fn version(
    pool: AnyPool,
    host: String,
    version: u64,
) -> Result<Option<RouteVersion>, StoreError> {
    let host = host.to_lowercase();
    let row = sqlx::query(
        "SELECT schema_version, data, created_at, author FROM proksi_route_versions \
         WHERE host = $1 AND revision = $2",
    )
    .bind(&host)
    .bind(to_i64(version)?)
    .fetch_optional(&pool)
    .await
    .map_err(unavailable)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let schema_version: i64 = row.try_get("schema_version").map_err(invalid)?;
    Ok(Some(RouteVersion {
        record: RouteRecord {
            host,
            schema_version: u32::try_from(schema_version).map_err(invalid)?,
            data: row.try_get("data").map_err(invalid)?,
            version,
            changed: 0,
        },
        at: to_u64(row.try_get("created_at").map_err(invalid)?)?,
        author: row.try_get("author").map_err(invalid)?,
    }))
}

pub(crate) async fn deleted(pool: AnyPool) -> Result<Vec<String>, StoreError> {
    let rows = sqlx::query(
        "SELECT DISTINCT v.host FROM proksi_route_versions v WHERE NOT EXISTS \
         (SELECT 1 FROM proksi_routes r WHERE r.host = v.host) ORDER BY v.host",
    )
    .fetch_all(&pool)
    .await
    .map_err(unavailable)?;
    rows.iter()
        .map(|row| row.try_get("host").map_err(invalid))
        .collect()
}
