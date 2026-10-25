//! Cursors in Redis: one script moves a namespace's cursors, each a key,
//! `proksi:cursor:{<namespace>}:<key>` (one hash slot per namespace, for clusters). Times are
//! Redis' own (`TIME`), in microseconds: nanoseconds since the epoch don't fit Lua's numbers.

use crate::route::StoreError;

const ADVANCE: &str = r"
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000000 + tonumber(t[2])
local ahead = {}
for i, key in ipairs(KEYS) do
    local time = math.max(tonumber(redis.call('GET', key) or '0'), now) + tonumber(ARGV[i])
    if time > now then
        redis.call('SET', key, string.format('%d', time), 'PX', math.ceil((time - now) / 1000) + 1)
    end
    ahead[i] = time - now
end
return ahead
";

pub(crate) fn advance(
    conn: &mut redis::Connection,
    namespace: &str,
    steps: &[(String, u64)],
) -> Result<Vec<u64>, StoreError> {
    if steps.is_empty() {
        return Ok(vec![]);
    }
    let script = redis::Script::new(ADVANCE);
    let mut invocation = script.prepare_invoke();
    for (key, step) in steps {
        invocation
            .key(format!("proksi:cursor:{{{namespace}}}:{key}"))
            .arg(*step);
    }
    invocation
        .invoke(conn)
        .map_err(|err| StoreError::Unavailable(err.to_string()))
}
