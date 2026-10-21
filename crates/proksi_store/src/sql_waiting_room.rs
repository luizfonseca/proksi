//! Waiting rooms in SQL (migration `0005_waiting_rooms.sql`). A call first touches its room's
//! row in `proksi_waiting_rooms`: the row stays locked until the call commits, so calls to a
//! room run one at a time on every replica.

use sqlx::{AnyConnection, AnyPool, Row};

use crate::{
    route::StoreError,
    waiting_room::{from_hex, hex, millis, new_key, Admission, RoomLimits},
};

const KEY_NAME: &str = "waiting_room";

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn invalid(err: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(err.to_string())
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(invalid)
}

fn to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}

pub(crate) async fn wait(
    pool: AnyPool,
    now_ms: &'static str,
    room: String,
    visitor: String,
    limits: RoomLimits,
) -> Result<Admission, StoreError> {
    let session = to_i64(millis(limits.session))?;
    let abandon = to_i64(millis(limits.abandon))?;
    let mut tx = pool.begin().await.map_err(unavailable)?;

    // Locks the room until the end
    sqlx::query(
        "INSERT INTO proksi_waiting_rooms (room, next_ticket) VALUES ($1, 0) \
         ON CONFLICT (room) DO UPDATE SET next_ticket = proksi_waiting_rooms.next_ticket",
    )
    .bind(&room)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    let now: i64 = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {now_ms} AS now")))
        .fetch_one(&mut *tx)
        .await
        .map_err(unavailable)?
        .try_get("now")
        .map_err(invalid)?;

    // Expired slots, and visitors who stopped waiting
    sqlx::query(
        "DELETE FROM proksi_waiting_room_visitors WHERE room = $1 \
         AND ((expires_at IS NOT NULL AND expires_at <= $2) \
           OR (expires_at IS NULL AND seen_at < $3))",
    )
    .bind(&room)
    .bind(now)
    .bind(now - abandon)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;

    let admission = visit(&mut tx, &room, &visitor, limits.active, now, session).await?;
    tx.commit().await.map_err(unavailable)?;
    Ok(admission)
}

/// Renews the visitor's slot, or places them, and lets them in if a slot is free for them
async fn visit(
    tx: &mut AnyConnection,
    room: &str,
    visitor: &str,
    active: u64,
    now: i64,
    session: i64,
) -> Result<Admission, StoreError> {
    let found = sqlx::query(
        "SELECT ticket, expires_at FROM proksi_waiting_room_visitors \
         WHERE room = $1 AND visitor = $2",
    )
    .bind(room)
    .bind(visitor)
    .fetch_optional(&mut *tx)
    .await
    .map_err(unavailable)?;
    let ticket = match found {
        Some(row)
            if row
                .try_get::<Option<i64>, _>("expires_at")
                .map_err(invalid)?
                .is_some() =>
        {
            admit(tx, room, visitor, now + session).await?;
            return Ok(Admission::Admitted);
        }
        Some(row) => {
            sqlx::query(
                "UPDATE proksi_waiting_room_visitors SET seen_at = $3 \
                 WHERE room = $1 AND visitor = $2",
            )
            .bind(room)
            .bind(visitor)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(unavailable)?;
            row.try_get::<i64, _>("ticket").map_err(invalid)?
        }
        None => place(tx, room, visitor, now).await?,
    };

    let active_now = count(tx, room, "expires_at IS NOT NULL", None).await?;
    let free = active.saturating_sub(active_now);
    // Visitors who came before get the free slots first
    let before = count(tx, room, "expires_at IS NULL AND ticket < $2", Some(ticket)).await?;
    if before < free {
        admit(tx, room, visitor, now + session).await?;
        return Ok(Admission::Admitted);
    }
    Ok(Admission::Waiting {
        position: before - free + 1,
        waiting: count(tx, room, "expires_at IS NULL", None).await?,
    })
}

/// A new visitor's ticket, at the end of the line
async fn place(
    tx: &mut AnyConnection,
    room: &str,
    visitor: &str,
    now: i64,
) -> Result<i64, StoreError> {
    let ticket: i64 = sqlx::query(
        "UPDATE proksi_waiting_rooms SET next_ticket = next_ticket + 1 WHERE room = $1 \
         RETURNING next_ticket",
    )
    .bind(room)
    .fetch_one(&mut *tx)
    .await
    .map_err(unavailable)?
    .try_get("next_ticket")
    .map_err(invalid)?;
    sqlx::query(
        "INSERT INTO proksi_waiting_room_visitors (room, visitor, ticket, expires_at, seen_at) \
         VALUES ($1, $2, $3, NULL, $4)",
    )
    .bind(room)
    .bind(visitor)
    .bind(ticket)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    Ok(ticket)
}

/// Gives the visitor a slot until `expires_at`
async fn admit(
    tx: &mut AnyConnection,
    room: &str,
    visitor: &str,
    expires_at: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE proksi_waiting_room_visitors SET expires_at = $3 WHERE room = $1 AND visitor = $2",
    )
    .bind(room)
    .bind(visitor)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// The room's visitors matching `filter`, which reads `$2` from `ticket`
async fn count(
    tx: &mut AnyConnection,
    room: &str,
    filter: &'static str,
    ticket: Option<i64>,
) -> Result<u64, StoreError> {
    let sql = format!(
        "SELECT COUNT(*) AS n FROM proksi_waiting_room_visitors WHERE room = $1 AND {filter}"
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(room);
    if let Some(ticket) = ticket {
        query = query.bind(ticket);
    }
    let row = query.fetch_one(&mut *tx).await.map_err(unavailable)?;
    to_u64(row.try_get("n").map_err(invalid)?)
}

/// The key of waiting rooms' cookies, set by the first replica asking for it
pub(crate) async fn key(pool: AnyPool) -> Result<[u8; 32], StoreError> {
    sqlx::query(
        "INSERT INTO proksi_keys (name, value) VALUES ($1, $2) ON CONFLICT (name) DO NOTHING",
    )
    .bind(KEY_NAME)
    .bind(hex(&new_key()?))
    .execute(&pool)
    .await
    .map_err(unavailable)?;
    let stored: String = sqlx::query("SELECT value FROM proksi_keys WHERE name = $1")
        .bind(KEY_NAME)
        .fetch_one(&pool)
        .await
        .map_err(unavailable)?
        .try_get("value")
        .map_err(invalid)?;
    from_hex(&stored).ok_or_else(|| invalid("the waiting rooms' key isn't a key"))
}
