//! Changes to a JSON body at a path, made on its bytes: the value (or member, or element) is
//! found with sonic-rs, which only reads what leads to it, and the bytes around it are copied
//! with the change in between. The rest of the body is never parsed into values, or written
//! again, so a change costs about a copy of the body.

use std::ops::Range;

use sonic_rs::PointerNode;

use super::path::{self, Step};

/// A change, compiled when the configuration loads
#[derive(Debug)]
pub(super) enum Edit {
    /// Sets the value at `path` to `value` (JSON): replacing it, or adding the member to an
    /// object that's there
    Set {
        path: Vec<PointerNode>,
        value: Vec<u8>,
        /// The last step's name, as JSON (`"role"`), to add the member
        key: Option<Vec<u8>>,
    },
    /// Removes the member or element at `path`
    Remove { path: Vec<PointerNode> },
}

impl Edit {
    pub(super) fn set(path: &str, value: &serde_json::Value) -> Result<Self, String> {
        let steps = path::parse(path)?;
        let key = match steps.last() {
            Some(Step::Key(key)) => Some(serde_json::to_vec(key).map_err(|err| err.to_string())?),
            _ => None,
        };
        Ok(Edit::Set {
            path: steps.iter().map(Step::node).collect(),
            value: serde_json::to_vec(value).map_err(|err| err.to_string())?,
            key,
        })
    }

    pub(super) fn remove(path: &str) -> Result<Self, String> {
        let steps = path::parse(path)?;
        Ok(Edit::Remove {
            path: steps.iter().map(Step::node).collect(),
        })
    }

    /// `body` changed, or `None` when the path isn't in it
    pub(super) fn apply(&self, body: &[u8]) -> Option<Vec<u8>> {
        match self {
            Edit::Set { path, value, key } => set(body, path, value, key.as_deref()),
            Edit::Remove { path } => remove(body, path),
        }
    }
}

fn set(body: &[u8], path: &[PointerNode], value: &[u8], key: Option<&[u8]>) -> Option<Vec<u8>> {
    if let Ok(found) = sonic_rs::get(body, path) {
        let at = within(body, found.as_raw_str().as_bytes())?;
        return Some(splice(body, at, value));
    }
    // A member that isn't there, of an object that is: added first
    let (_, parent) = path.split_last()?;
    let object = container(body, parent)?;
    // An object to its end: one found inside the body is (sonic-rs found where it ends), and
    // the body's must end with `}` (`{"a": ` is no object to add to). Checked without reading
    // the members, which can be most of the body.
    let members = body.get(object.clone())?;
    if members.first() != Some(&b'{') || members.last() != Some(&b'}') {
        return None;
    }
    let after_brace = object.start + 1;
    let empty = first_non_space(body, after_brace) == Some(b'}');
    let mut member = Vec::with_capacity(key?.len() + 1 + value.len() + 1);
    member.extend_from_slice(key?);
    member.push(b':');
    member.extend_from_slice(value);
    if !empty {
        member.push(b',');
    }
    Some(splice(body, after_brace..after_brace, &member))
}

fn remove(body: &[u8], path: &[PointerNode]) -> Option<Vec<u8>> {
    let (last, parent) = path.split_last()?;
    let parent = container(body, parent)?;
    let cut = match last {
        PointerNode::Key(key) => member(body, parent, key)?,
        PointerNode::Index(index) => element(body, parent, *index)?,
    };
    Some(splice(body, cut, b""))
}

/// The bytes to cut to remove the member `key` of the object at `object`, with a comma
fn member(body: &[u8], object: Range<usize>, key: &str) -> Option<Range<usize>> {
    let mut previous_end = None;
    for entry in sonic_rs::to_object_iter(body.get(object.clone())?) {
        let (name, value) = entry.ok()?;
        let value = within(body, value.as_raw_str().as_bytes())?;
        if &*name != key {
            previous_end = Some(value.end);
            continue;
        }
        // `,"key":value` after another member, `"key":value,` before one, or the only one
        return Some(match previous_end {
            Some(end) => end..value.end,
            None => object.start + 1..next_after_comma(body, value.end).unwrap_or(value.end),
        });
    }
    None
}

/// The bytes to cut to remove element `index` of the array at `array`, with a comma
fn element(body: &[u8], array: Range<usize>, index: usize) -> Option<Range<usize>> {
    let mut elements = sonic_rs::to_array_iter(body.get(array.clone())?)
        .map(|element| within(body, element.ok()?.as_raw_str().as_bytes()));
    let mut previous_end = None;
    for _ in 0..index {
        previous_end = Some(elements.next()??.end);
    }
    let removed = elements.next()??;
    Some(match previous_end {
        Some(end) => end..removed.end,
        None => array.start + 1..next_after_comma(body, removed.end).unwrap_or(removed.end),
    })
}

/// Where the object or array at `path` is in `body` (the whole body for no path)
fn container(body: &[u8], path: &[PointerNode]) -> Option<Range<usize>> {
    if !path.is_empty() {
        let found = sonic_rs::get(body, path).ok()?;
        return within(body, found.as_raw_str().as_bytes());
    }
    let start = body.iter().position(|byte| !byte.is_ascii_whitespace())?;
    let end = body.iter().rposition(|byte| !byte.is_ascii_whitespace())? + 1;
    Some(start..end)
}

/// Where `part`, a slice of `body`, is in it
fn within(body: &[u8], part: &[u8]) -> Option<Range<usize>> {
    let start = (part.as_ptr() as usize).checked_sub(body.as_ptr() as usize)?;
    let end = start.checked_add(part.len())?;
    (end <= body.len()).then_some(start..end)
}

fn first_non_space(body: &[u8], from: usize) -> Option<u8> {
    body.get(from..)?
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

/// After a value ending at `end`: where the next member or element starts, past the comma
fn next_after_comma(body: &[u8], end: usize) -> Option<usize> {
    let rest = body.get(end..)?;
    let comma = rest.iter().position(|byte| !byte.is_ascii_whitespace())?;
    if rest.get(comma) != Some(&b',') {
        return None;
    }
    let next = rest
        .get(comma + 1..)?
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())?;
    Some(end + comma + 1 + next)
}

/// `body` with `at` replaced by `with`
fn splice(body: &[u8], at: Range<usize>, with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() - at.len() + with.len());
    out.extend_from_slice(body.get(..at.start).unwrap_or_default());
    out.extend_from_slice(with);
    out.extend_from_slice(body.get(at.end..).unwrap_or_default());
    out
}

/// Checks the JSON of a changed body parses back, for tests
#[cfg(test)]
pub(super) fn parses(body: &[u8]) -> sonic_rs::Value {
    sonic_rs::from_slice(body).unwrap()
}

#[cfg(test)]
mod tests {
    use super::{parses, Edit};

    fn set(path: &str, value: serde_json::Value, body: &str) -> Option<String> {
        let edit = Edit::set(path, &value).unwrap();
        edit.apply(body.as_bytes())
            .map(|out| String::from_utf8(out).unwrap())
    }

    fn remove(path: &str, body: &str) -> Option<String> {
        let edit = Edit::remove(path).unwrap();
        edit.apply(body.as_bytes())
            .map(|out| String::from_utf8(out).unwrap())
    }

    const BODY: &str = r#"{"user": {"id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#;

    #[test]
    fn test_setting_a_value_that_is_there() {
        assert_eq!(
            set("$.user.role", "guest".into(), BODY).as_deref(),
            Some(
                r#"{"user": {"id": 42, "role": "guest"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#
            )
        );
        let out = set("$.items[1].id", serde_json::json!({ "a": [1, 2] }), BODY).unwrap();
        assert!(out.contains(r#"{"id": {"a":[1,2]}}"#), "{out}");
    }

    #[test]
    fn test_setting_a_member_that_isnt_there() {
        assert_eq!(
            set("$.user.plan", "free".into(), BODY).as_deref(),
            Some(
                r#"{"user": {"plan":"free","id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#
            )
        );
        assert_eq!(
            set("$.a", 1.into(), " { } ").as_deref(),
            Some(r#" {"a":1 } "#)
        );
        assert_eq!(
            set("$['a \"b\"']", 1.into(), "{}").as_deref(),
            Some(r#"{"a \"b\"":1}"#)
        );
        // Not in a missing object, nor past an array's end, nor in a value that isn't an object
        assert_eq!(set("$.missing.a", 1.into(), BODY), None);
        assert_eq!(set("$.items[9]", 1.into(), BODY), None);
        assert_eq!(set("$.debug.a", 1.into(), BODY), None);
    }

    #[test]
    fn test_removing_members() {
        // First, middle, last, only
        assert_eq!(
            remove("$.user", r#"{"user": 1, "b": 2}"#).as_deref(),
            Some(r#"{"b": 2}"#)
        );
        assert_eq!(
            remove("$.b", r#"{"a": 1, "b": 2, "c": 3}"#).as_deref(),
            Some(r#"{"a": 1, "c": 3}"#)
        );
        assert_eq!(
            remove("$.c", r#"{"a": 1, "c": 3}"#).as_deref(),
            Some(r#"{"a": 1}"#)
        );
        assert_eq!(remove("$.a", r#"{ "a" : 1 }"#).as_deref(), Some("{ }"));
        assert_eq!(remove("$.user.role", BODY).map(|out| parses(out.as_bytes())), Some(parses(br#"{"user": {"id": 42}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#)));
        // Keys are compared unescaped
        assert_eq!(
            remove("$['a\"b']", r#"{"a\"b": 1, "c": 2}"#).as_deref(),
            Some(r#"{"c": 2}"#)
        );
        assert_eq!(remove("$.missing", BODY), None);
    }

    #[test]
    fn test_removing_elements() {
        assert_eq!(remove("$[0]", "[1, 2, 3]").as_deref(), Some("[2, 3]"));
        assert_eq!(remove("$[1]", "[1, 2, 3]").as_deref(), Some("[1, 3]"));
        assert_eq!(remove("$[2]", "[1, 2, 3]").as_deref(), Some("[1, 2]"));
        assert_eq!(remove("$[0]", "[ 1 ]").as_deref(), Some("[ ]"));
        assert_eq!(remove("$[3]", "[1, 2, 3]"), None);
        let out = remove("$.items[1]", BODY).unwrap();
        assert_eq!(parses(out.as_bytes()), parses(br#"{"user": {"id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": 3}]}"#));
    }

    #[test]
    fn test_bodies_that_arent_json_are_left_alone() {
        for body in ["", "not json", "[1, 2", r#"{"user": "#] {
            assert_eq!(set("$.user", 1.into(), body), None, "{body}");
            assert_eq!(remove("$.user", body), None, "{body}");
        }
    }
}
