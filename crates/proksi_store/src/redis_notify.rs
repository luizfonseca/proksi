//! `notify` blocks in Redis:
//! - `proksi:notify`: each block's data, by name
//! - `proksi:notify:versions`: the revision each block was saved at, by name
//! - `proksi:notify:revision`: the blocks' revision
//!
//! Reads and changes are scripts: what a change expects, the change and the revision are atomic.

use crate::{
    notify::{NotifyRecord, NotifySnapshot},
    route::{Expect, StoreError, Write},
};

const KEYS: [&str; 3] = [
    "proksi:notify",
    "proksi:notify:versions",
    "proksi:notify:revision",
];

/// Every block and the revision, together: `{revision, name, data, version, name, ...}`
const READ: &str = r"
local answer = {redis.call('GET', KEYS[3]) or '0'}
local blocks = redis.call('HGETALL', KEYS[1])
for at = 1, #blocks, 2 do
  local name = blocks[at]
  table.insert(answer, name)
  table.insert(answer, blocks[at + 1])
  table.insert(answer, redis.call('HGET', KEYS[2], name) or '0')
end
return answer
";

/// Returns `{1, version}`, the block's version, from the script when it isn't as the change
/// expects (`ARGV[1]`: `any`, `absent` or `version`; `ARGV[2]`: the version; `ARGV[3]`: the name)
const CHECK: &str = r"
local saved = tonumber(redis.call('HGET', KEYS[2], ARGV[3]) or '0')
local present = redis.call('HEXISTS', KEYS[1], ARGV[3]) == 1
if (ARGV[1] == 'absent' and present) or (ARGV[1] == 'version' and tonumber(ARGV[2]) ~= saved) then
  return {1, saved}
end
";

/// `ARGV[4]`: the data
const PUT: &str = r"
local revision = redis.call('INCR', KEYS[3])
redis.call('HSET', KEYS[1], ARGV[3], ARGV[4])
redis.call('HSET', KEYS[2], ARGV[3], revision)
return {0, revision}
";

const DELETE: &str = r"
if redis.call('HDEL', KEYS[1], ARGV[3]) == 0 then
  return {0, tonumber(redis.call('GET', KEYS[3]) or '0')}
end
redis.call('HDEL', KEYS[2], ARGV[3])
return {0, redis.call('INCR', KEYS[3])}
";

fn unavailable(err: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(err.to_string())
}

fn number(text: &str) -> Result<u64, StoreError> {
    text.parse()
        .map_err(|_| StoreError::Invalid(format!("not a revision: {text:?}")))
}

pub(crate) fn revision(conn: &mut redis::Connection) -> Result<u64, StoreError> {
    let revision: Option<u64> = redis::cmd("GET")
        .arg(KEYS[2])
        .query(conn)
        .map_err(unavailable)?;
    Ok(revision.unwrap_or(0))
}

pub(crate) fn blocks(conn: &mut redis::Connection) -> Result<NotifySnapshot, StoreError> {
    let script = redis::Script::new(READ);
    let mut invocation = script.prepare_invoke();
    for key in KEYS {
        invocation.key(key);
    }
    let answer: Vec<String> = invocation.invoke(conn).map_err(unavailable)?;
    let Some((revision, rest)) = answer.split_first() else {
        return Ok(NotifySnapshot::default());
    };
    let (blocks, left) = rest.as_chunks::<3>();
    if !left.is_empty() {
        return Err(StoreError::Invalid("a block without its data".into()));
    }
    let mut blocks = blocks
        .iter()
        .map(|[name, data, version]| {
            Ok(NotifyRecord {
                name: name.clone(),
                data: data.clone(),
                version: number(version)?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    blocks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(NotifySnapshot {
        revision: number(revision)?,
        blocks,
    })
}

/// Runs a change to the block `name`, with what it expects and `data` (for a put)
fn change(
    conn: &mut redis::Connection,
    body: &str,
    name: &str,
    write: &Write,
    data: Option<&str>,
) -> Result<u64, StoreError> {
    let script = redis::Script::new(&format!("{CHECK}\n{body}"));
    let mut invocation = script.prepare_invoke();
    for key in KEYS {
        invocation.key(key);
    }
    let (kind, value) = match write.expect {
        Expect::Any => ("any", 0),
        Expect::Absent => ("absent", 0),
        Expect::Version(version) => ("version", version),
    };
    invocation.arg(kind).arg(value).arg(name);
    if let Some(data) = data {
        invocation.arg(data);
    }
    let result: Vec<u64> = invocation.invoke(conn).map_err(unavailable)?;
    match result.as_slice() {
        [0, revision] => Ok(*revision),
        [1, version] => Err(StoreError::Conflict { version: *version }),
        _ => Err(StoreError::Invalid(format!("unexpected answer {result:?}"))),
    }
}

pub(crate) fn put(
    conn: &mut redis::Connection,
    block: &NotifyRecord,
    write: &Write,
) -> Result<u64, StoreError> {
    change(conn, PUT, &block.name, write, Some(&block.data))
}

pub(crate) fn delete(
    conn: &mut redis::Connection,
    name: &str,
    write: &Write,
) -> Result<u64, StoreError> {
    change(conn, DELETE, name, write, None)
}
