//! `notify` blocks in SQL (migration `0006_notify.sql`). A change first increments their
//! revision: the row stays locked until the change commits, so changes run one at a time and
//! what one expects can't change under it.

use sqlx::{AnyConnection, AnyPool, Row};

use crate::{
    notify::{self, NotifyRecord, NotifySnapshot},
    route::{StoreError, Write},
};

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn invalid(err: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(err.to_string())
}

fn to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or_default()
}

pub(crate) async fn revision<'e, E>(executor: E) -> Result<u64, StoreError>
where
    E: sqlx::Executor<'e, Database = sqlx::Any>,
{
    let row = sqlx::query("SELECT value FROM proksi_meta WHERE key = 'notify_revision'")
        .fetch_one(executor)
        .await
        .map_err(unavailable)?;
    to_u64(row.try_get("value").map_err(invalid)?)
}

/// The revision is read first: blocks changed in between are read again at the next revision
pub(crate) async fn blocks(pool: AnyPool) -> Result<NotifySnapshot, StoreError> {
    let revision = revision(&pool).await?;
    let rows = sqlx::query("SELECT name, data, version FROM proksi_notify ORDER BY name")
        .fetch_all(&pool)
        .await
        .map_err(unavailable)?;
    let blocks = rows
        .iter()
        .map(|row| {
            Ok(NotifyRecord {
                name: row.try_get("name").map_err(invalid)?,
                data: row.try_get("data").map_err(invalid)?,
                version: to_u64(row.try_get("version").map_err(invalid)?)?,
            })
        })
        .collect::<Result<_, StoreError>>()?;
    Ok(NotifySnapshot { revision, blocks })
}

/// Increments the blocks' revision, locking it until the transaction ends
async fn next_revision(tx: &mut AnyConnection) -> Result<u64, StoreError> {
    let row = sqlx::query(
        "UPDATE proksi_meta SET value = value + 1 WHERE key = 'notify_revision' RETURNING value",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(unavailable)?;
    to_u64(row.try_get("value").map_err(invalid)?)
}

/// Refuses a change the block doesn't expect. Returns whether the block is in the store.
async fn check(tx: &mut AnyConnection, name: &str, write: &Write) -> Result<bool, StoreError> {
    let version = sqlx::query("SELECT version FROM proksi_notify WHERE name = $1")
        .bind(name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(unavailable)?
        .map(|row| {
            row.try_get::<i64, _>("version")
                .map_err(invalid)
                .and_then(to_u64)
        })
        .transpose()?;
    notify::check(write.expect, version)?;
    Ok(version.is_some())
}

pub(crate) async fn put(
    pool: AnyPool,
    block: NotifyRecord,
    write: Write,
) -> Result<u64, StoreError> {
    let mut tx = pool.begin().await.map_err(unavailable)?;
    let revision = next_revision(&mut tx).await?;
    check(&mut tx, &block.name, &write).await?;
    sqlx::query(
        "INSERT INTO proksi_notify (name, data, version, updated_at) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (name) DO UPDATE SET data = excluded.data, version = excluded.version, \
         updated_at = excluded.updated_at",
    )
    .bind(&block.name)
    .bind(&block.data)
    .bind(i64::try_from(revision).map_err(invalid)?)
    .bind(unix_timestamp())
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(revision)
}

pub(crate) async fn delete(pool: AnyPool, name: String, write: Write) -> Result<u64, StoreError> {
    let mut tx = pool.begin().await.map_err(unavailable)?;
    let revision = next_revision(&mut tx).await?;
    if !check(&mut tx, &name, &write).await? {
        // Nothing to remove: the revision stays as it was
        return Ok(revision - 1);
    }
    sqlx::query("DELETE FROM proksi_notify WHERE name = $1")
        .bind(&name)
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(revision)
}
