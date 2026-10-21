//! Waiting rooms in Redis, each changed by one script, so replicas see a single queue. A room
//! `<room>` is kept in four keys, `proksi:room:{<room>}:...` (one hash slot, for clusters):
//! `active` (who holds a slot, by when it expires), `queue` (who waits, by ticket), `seen`
//! (who waits, by when they last called) and `next` (the last ticket). Times are Redis' own
//! (`TIME`), in milliseconds; a room nobody calls expires.

use redis::Commands;

use crate::{
    route::StoreError,
    waiting_room::{from_hex, hex, millis, new_key, Admission, RoomLimits},
};

const KEY_KEY: &str = "proksi:waiting-room:key";

const WAIT: &str = r"
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local visitor, limit = ARGV[1], tonumber(ARGV[2])
local session, abandon = tonumber(ARGV[3]), tonumber(ARGV[4])
local active, queue, seen, next = KEYS[1], KEYS[2], KEYS[3], KEYS[4]

-- Expired slots, and visitors who stopped waiting
redis.call('ZREMRANGEBYSCORE', active, '-inf', now)
local gone = redis.call('ZRANGEBYSCORE', seen, '-inf', '(' .. (now - abandon))
for _, left in ipairs(gone) do
    redis.call('ZREM', queue, left)
    redis.call('ZREM', seen, left)
end

local function keep()
    for _, key in ipairs(KEYS) do
        redis.call('PEXPIRE', key, session + abandon)
    end
end

if redis.call('ZSCORE', active, visitor) then
    redis.call('ZADD', active, now + session, visitor)
    keep()
    return {1, 0, 0}
end
if not redis.call('ZSCORE', queue, visitor) then
    redis.call('ZADD', queue, redis.call('INCR', next), visitor)
end
redis.call('ZADD', seen, now, visitor)

-- Visitors who came before get the free slots first
local free = limit - redis.call('ZCARD', active)
local before = redis.call('ZRANK', queue, visitor)
if before < free then
    redis.call('ZREM', queue, visitor)
    redis.call('ZREM', seen, visitor)
    redis.call('ZADD', active, now + session, visitor)
    keep()
    return {1, 0, 0}
end
keep()
return {0, before - free + 1, redis.call('ZCARD', queue)}
";

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

pub(crate) fn wait(
    conn: &mut redis::Connection,
    room: &str,
    visitor: &str,
    limits: RoomLimits,
) -> Result<Admission, StoreError> {
    let key = |name: &str| format!("proksi:room:{{{room}}}:{name}");
    let (admitted, position, waiting): (u64, u64, u64) = redis::Script::new(WAIT)
        .key(key("active"))
        .key(key("queue"))
        .key(key("seen"))
        .key(key("next"))
        .arg(visitor)
        .arg(limits.active)
        .arg(millis(limits.session).max(1))
        .arg(millis(limits.abandon).max(1))
        .invoke(conn)
        .map_err(unavailable)?;
    Ok(if admitted == 1 {
        Admission::Admitted
    } else {
        Admission::Waiting { position, waiting }
    })
}

/// The key of waiting rooms' cookies, set by the first replica asking for it
pub(crate) fn key(conn: &mut redis::Connection) -> Result<[u8; 32], StoreError> {
    let new = hex(&new_key()?);
    // Kept if another replica set it first
    let _: Option<String> = redis::cmd("SET")
        .arg(KEY_KEY)
        .arg(&new)
        .arg("NX")
        .query(conn)
        .map_err(unavailable)?;
    let stored: String = conn.get(KEY_KEY).map_err(unavailable)?;
    from_hex(&stored).ok_or_else(|| StoreError::Invalid(format!("{KEY_KEY} isn't a key")))
}
