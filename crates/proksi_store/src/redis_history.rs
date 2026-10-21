//! Versions and history of hosts in Redis.
//!
//! For each host:
//! - `proksi:route:<host>`: its active definition, read by every replica (and by older Proksi
//!   versions, which know nothing else)
//! - `proksi:route:<host>:meta`: `version` (the active version, 0 for none) and `changed` (the
//!   revision of its last change)
//! - `proksi:route:<host>:<revision>`: the version saved at that revision (`schema_version`,
//!   `data`, `at`, `author`)
//! - `proksi:route:<host>:versions`: the revisions of its versions, sorted
//! - `proksi:route:<host>:history`: its changes, newest first (JSON)
//!
//! and `proksi:routes:history`, the hosts that have versions. Each change is one script: what
//! it expects of the host, the change and the store's revision are atomic.

use redis::Commands;

use crate::route::{
    Expect, HistoryEntry, RouteRecord, RouteVersion, StoreError, Write,
};

const ROUTES_SET_KEY: &str = "proksi:routes";
const REVISION_KEY: &str = "proksi:revision";
const HISTORY_HOSTS_KEY: &str = "proksi:routes:history";

/// The keys of a host's definition, versions and history, in the order the scripts take them
fn keys(host: &str) -> [String; 7] {
    let prefix = format!("proksi:route:{host}");
    [
        prefix.clone(),
        ROUTES_SET_KEY.to_string(),
        REVISION_KEY.to_string(),
        format!("{prefix}:meta"),
        format!("{prefix}:versions"),
        format!("{prefix}:history"),
        HISTORY_HOSTS_KEY.to_string(),
    ]
}

/// The key of a host's version
fn version_key(host: &str, version: u64) -> String {
    format!("proksi:route:{host}:{version}")
}

/// Checks what a change expects of the host (`ARGV[1]`, `ARGV[2]`): returns `{1, changed,
/// present}` from the script when it isn't so
const CHECK: &str = r"
local changed = tonumber(redis.call('HGET', KEYS[4], 'changed') or '0')
local present = redis.call('EXISTS', KEYS[1]) == 1
local kind = ARGV[1]
if (kind == 'absent' and present) or (kind == 'changed' and tonumber(ARGV[2]) ~= changed) then
  return {1, changed, present and 1 or 0}
end
";

/// Records a change (`entry`, without its revision) at `revision`, and keeps the last
/// `ARGV[3]` changes and versions (and the active version)
const RECORD: &str = r"
local function record(revision, entry)
  entry['revision'] = revision
  redis.call('LPUSH', KEYS[6], cjson.encode(entry))
  local keep = tonumber(ARGV[3])
  redis.call('LTRIM', KEYS[6], 0, keep - 1)
  redis.call('HSET', KEYS[4], 'changed', revision)
  local active = redis.call('HGET', KEYS[4], 'version')
  local versions = redis.call('ZRANGE', KEYS[5], 0, -1)
  local excess = #versions - keep
  for _, version in ipairs(versions) do
    if excess <= 0 then break end
    if version ~= active then
      redis.call('DEL', ARGV[4] .. version)
      redis.call('ZREM', KEYS[5], version)
      excess = excess - 1
    end
  end
end
";

/// `ARGV`: expect kind, expect value, versions kept, version key prefix, at, author,
/// schema version, data
const PUT: &str = r"
local revision = redis.call('INCR', KEYS[3])
local key = ARGV[4] .. revision
redis.call('HSET', key, 'schema_version', ARGV[7], 'data', ARGV[8], 'at', ARGV[5], 'author', ARGV[6])
redis.call('ZADD', KEYS[5], revision, revision)
redis.call('SET', KEYS[1], ARGV[8])
redis.call('SADD', KEYS[2], ARGV[9])
redis.call('SADD', KEYS[7], ARGV[9])
redis.call('HSET', KEYS[4], 'version', revision)
local author = cjson.null
if ARGV[6] ~= '' then author = ARGV[6] end
record(revision, {at = tonumber(ARGV[5]), author = author, change = 'saved'})
return {0, revision}
";

/// `ARGV`: expect kind, expect value, versions kept, version key prefix, at, author, host
const DELETE: &str = r"
if redis.call('EXISTS', KEYS[1]) == 0 then
  return {0, tonumber(redis.call('GET', KEYS[3]) or '0')}
end
redis.call('DEL', KEYS[1])
redis.call('SREM', KEYS[2], ARGV[7])
redis.call('HSET', KEYS[4], 'version', 0)
local revision = redis.call('INCR', KEYS[3])
local author = cjson.null
if ARGV[6] ~= '' then author = ARGV[6] end
record(revision, {at = tonumber(ARGV[5]), author = author, change = 'deleted'})
return {0, revision}
";

/// `ARGV`: expect kind, expect value, versions kept, version key prefix, at, author, host,
/// version to roll back to
const ROLLBACK: &str = r"
local key = ARGV[4] .. ARGV[8]
local data = redis.call('HGET', key, 'data')
if not data then
  return {2, 0, 0}
end
redis.call('SET', KEYS[1], data)
redis.call('SADD', KEYS[2], ARGV[7])
redis.call('HSET', KEYS[4], 'version', ARGV[8])
local revision = redis.call('INCR', KEYS[3])
local author = cjson.null
if ARGV[6] ~= '' then author = ARGV[6] end
record(revision, {at = tonumber(ARGV[5]), author = author, change = 'rolled_back', to = tonumber(ARGV[8])})
return {0, revision}
";

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn invalid(err: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(err.to_string())
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Runs a change script on `host`, with what it expects, `keep` versions, and `extra`
/// arguments after the author
fn change(
    conn: &mut redis::Connection,
    body: &str,
    host: &str,
    write: &Write,
    keep: usize,
    extra: &[String],
) -> Result<u64, StoreError> {
    let script = redis::Script::new(&format!("{RECORD}\n{CHECK}\n{body}"));
    let mut invocation = script.prepare_invoke();
    for key in keys(host) {
        invocation.key(key);
    }
    let (kind, value) = match write.expect {
        Expect::Any => ("any", 0),
        Expect::Absent => ("absent", 0),
        Expect::Changed(changed) => ("changed", changed),
    };
    invocation
        .arg(kind)
        .arg(value)
        .arg(keep)
        .arg(format!("proksi:route:{host}:"))
        .arg(unix_timestamp())
        .arg(write.author.as_deref().unwrap_or_default());
    for arg in extra {
        invocation.arg(arg);
    }
    let result: Vec<u64> = invocation.invoke(conn).map_err(unavailable)?;
    match result.as_slice() {
        [0, revision] => Ok(*revision),
        [1, changed, present] => Err(StoreError::Conflict {
            changed: *changed,
            present: *present == 1,
        }),
        _ => Err(StoreError::Invalid(format!("unexpected answer {result:?}"))),
    }
}

pub(crate) fn put(
    conn: &mut redis::Connection,
    route: &RouteRecord,
    write: &Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = route.host.to_lowercase();
    let extra = [
        route.schema_version.to_string(),
        route.data.clone(),
        host.clone(),
    ];
    change(conn, PUT, &host, write, keep, &extra)
}

pub(crate) fn delete(
    conn: &mut redis::Connection,
    host: &str,
    write: &Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = host.to_lowercase();
    let extra = [host.clone()];
    change(conn, DELETE, &host, write, keep, &extra)
}

pub(crate) fn rollback(
    conn: &mut redis::Connection,
    host: &str,
    to: u64,
    write: &Write,
    keep: usize,
) -> Result<u64, StoreError> {
    let host = host.to_lowercase();
    let extra = [host.clone(), to.to_string()];
    // The version is checked before what the change expects: a missing version isn't a
    // conflict
    let exists: bool = conn.exists(version_key(&host, to)).map_err(unavailable)?;
    if !exists {
        return Err(StoreError::NoVersion(to));
    }
    change(conn, ROLLBACK, &host, write, keep, &extra).map_err(|err| match err {
        StoreError::Invalid(_) => StoreError::NoVersion(to),
        err => err,
    })
}

pub(crate) fn history(
    conn: &mut redis::Connection,
    host: &str,
) -> Result<Vec<HistoryEntry>, StoreError> {
    let host = host.to_lowercase();
    let entries: Vec<String> = conn
        .lrange(format!("proksi:route:{host}:history"), 0, -1)
        .map_err(unavailable)?;
    entries
        .iter()
        .map(|entry| serde_json::from_str(entry).map_err(invalid))
        .collect()
}

pub(crate) fn version(
    conn: &mut redis::Connection,
    host: &str,
    version: u64,
) -> Result<Option<RouteVersion>, StoreError> {
    let host = host.to_lowercase();
    let fields: std::collections::HashMap<String, String> =
        conn.hgetall(version_key(&host, version)).map_err(unavailable)?;
    let (Some(data), Some(schema_version)) = (fields.get("data"), fields.get("schema_version"))
    else {
        return Ok(None);
    };
    let author = fields.get("author").filter(|author| !author.is_empty());
    Ok(Some(RouteVersion {
        record: RouteRecord {
            host,
            schema_version: schema_version.parse().map_err(invalid)?,
            data: data.clone(),
            version,
            changed: 0,
        },
        at: fields
            .get("at")
            .and_then(|at| at.parse().ok())
            .unwrap_or_default(),
        author: author.cloned(),
    }))
}

pub(crate) fn deleted(conn: &mut redis::Connection) -> Result<Vec<String>, StoreError> {
    let mut hosts: Vec<String> = conn
        .sdiff(&[HISTORY_HOSTS_KEY, ROUTES_SET_KEY])
        .map_err(unavailable)?;
    hosts.sort();
    Ok(hosts)
}

/// The active version and last change of each host (0 when unknown: saved before versions
/// were kept)
pub(crate) fn metas(
    conn: &mut redis::Connection,
    hosts: &[String],
) -> Result<Vec<(u64, u64)>, StoreError> {
    if hosts.is_empty() {
        return Ok(vec![]);
    }
    let mut pipe = redis::pipe();
    for host in hosts {
        pipe.cmd("HMGET")
            .arg(format!("proksi:route:{host}:meta"))
            .arg("version")
            .arg("changed");
    }
    let metas: Vec<(Option<u64>, Option<u64>)> = pipe.query(conn).map_err(unavailable)?;
    Ok(metas
        .into_iter()
        .map(|(version, changed)| (version.unwrap_or_default(), changed.unwrap_or_default()))
        .collect())
}
