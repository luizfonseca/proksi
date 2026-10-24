//! Rate limits in Redis: one script per block counts its clients' requests, a client being one
//! key, `proksi:rl:{<block>}:<client>` (one hash slot per block, for clusters), holding its
//! arrival time. Times are Redis' own (`TIME`), in microseconds: nanoseconds since the epoch
//! don't fit Lua's numbers.

use crate::{
    rate_limit::{Rate, Used},
    route::StoreError,
};

const COUNT: &str = r"
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000000 + tonumber(t[2])
local interval = math.max(tonumber(ARGV[1]), 1)
local ahead = {}
for i, key in ipairs(KEYS) do
    local stored = tonumber(redis.call('GET', key) or '0')
    local arrival = math.max(stored, now) + tonumber(ARGV[i + 1]) * interval
    redis.call('SET', key, string.format('%d', arrival), 'PX', math.ceil((arrival - now) / 1000) + 1)
    ahead[i] = arrival - now
end
return ahead
";

pub(crate) fn count(
    conn: &mut redis::Connection,
    block: &str,
    rate: Rate,
    used: &[Used],
) -> Result<Vec<u64>, StoreError> {
    if used.is_empty() {
        return Ok(vec![]);
    }
    let script = redis::Script::new(COUNT);
    let mut invocation = script.prepare_invoke();
    invocation.arg(rate.interval_us);
    for used in used {
        invocation
            .key(format!("proksi:rl:{{{block}}}:{}", used.client))
            .arg(used.requests);
    }
    invocation
        .invoke(conn)
        .map_err(|err| StoreError::Unavailable(err.to_string()))
}
