//! Changes to a JSON body made as it streams through, a part at a time. Each byte is read once,
//! in order, and passed on, dropped, or held back; a part without changes is passed on as it is.
//! Only the objects and arrays on the way to a path are followed: others are passed over,
//! counting their brackets. A part can end anywhere (in a key, a string, a number), and the
//! next one goes on from there.
//!
//! - `set` replaces a value, or adds the member to its object, before its `}`.
//! - `remove` removes a member, or an element of an array, with its comma.
//!
//! In the objects and arrays it follows, a comma and the key after it are held back until the
//! member is kept or removed, so the result stays valid JSON. A body that isn't JSON is passed
//! on as it comes from where it stops being JSON (what was changed before it, already sent,
//! stays changed).

use bytes::{Bytes, BytesMut};

use super::path::Step;

/// What's done at a path
#[derive(Debug, Clone)]
pub(super) enum Action {
    /// The value, as JSON
    Set(Vec<u8>),
    Remove,
}

#[derive(Debug, Clone)]
pub(super) struct Target {
    pub(super) steps: Vec<Step>,
    pub(super) action: Action,
    /// The last step's name as JSON (`"role"`), to add the member when it's missing
    pub(super) key: Option<Vec<u8>>,
}

/// The editor of one body, kept from a part to the next
#[derive(Debug, Default)]
pub(super) struct Editor {
    /// The objects and arrays being followed, outermost first
    frames: Vec<Frame>,
    read: Read,
    /// What's expected next, between tokens
    next: Next,
    /// Bytes held back (a comma, the key after it) until their member is kept or removed
    held: Vec<u8>,
    holding: bool,
    /// The current value is dropped (its member removed, or its value replaced)
    dropping: bool,
    /// The key being read, as written
    key: Vec<u8>,
    /// Not JSON: the rest passes as it comes
    passing: bool,
}

#[derive(Debug)]
struct Frame {
    object: bool,
    /// The targets this container leads to
    targets: Vec<usize>,
    /// The current element's index (arrays)
    index: usize,
    /// Members or elements kept so far: the next one kept follows a comma
    kept: usize,
    /// The target set at the current member, whose value is written instead of its own
    setting: Option<usize>,
    /// Targets of its members that were found (the others are added before `}`)
    found: Vec<usize>,
}

/// Inside a token
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Read {
    #[default]
    Between,
    Key {
        escaped: bool,
    },
    Text {
        escaped: bool,
    },
    Literal,
    /// An object or array that isn't followed
    Over {
        depth: usize,
        text: bool,
        escaped: bool,
    },
}

/// What's expected between tokens
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Next {
    #[default]
    Value,
    KeyOrEnd,
    Colon,
    After,
    Done,
}

/// Where the edited part is written: runs of the part as they are, and new bytes between them
struct Output<'a> {
    part: &'a Bytes,
    out: Option<BytesMut>,
    /// Where the run of bytes passed on started
    run: Option<usize>,
}

impl<'a> Output<'a> {
    fn new(part: &'a Bytes) -> Self {
        Self {
            part,
            out: None,
            run: None,
        }
    }

    fn pass(&mut self, at: usize) {
        self.run.get_or_insert(at);
    }

    /// The run so far ends before `at`
    fn end_run(&mut self, at: usize) {
        let Some(start) = self.run.take() else {
            return;
        };
        let run = self.part.get(start..at).unwrap_or_default();
        let out = self
            .out
            .get_or_insert_with(|| BytesMut::with_capacity(self.part.len() + 64));
        out.extend_from_slice(run);
    }

    fn write(&mut self, at: usize, bytes: &[u8]) {
        self.end_run(at);
        let out = self
            .out
            .get_or_insert_with(|| BytesMut::with_capacity(self.part.len() + 64));
        out.extend_from_slice(bytes);
    }

    fn done(mut self) -> Bytes {
        if self.out.is_none() && self.run == Some(0) {
            return self.part.clone();
        }
        let end = self.part.len();
        self.end_run(end);
        self.out.map(BytesMut::freeze).unwrap_or_default()
    }
}

impl Editor {
    /// The part of the body to send for `part`, `last` when it's the body's end
    pub(super) fn edit(&mut self, targets: &[Target], part: &Bytes, last: bool) -> Bytes {
        let mut out = Output::new(part);
        let mut at = 0;
        while let Some(byte) = part.get(at).copied() {
            if self.passing {
                out.pass(at);
                break;
            }
            at = match self.read {
                // Runs of bytes, read without stopping at each
                Read::Over { .. } => self.over_run(part, at, &mut out),
                Read::Text { .. } => self.text_run(part, at, &mut out),
                _ => {
                    self.step(targets, byte, at, &mut out);
                    at + 1
                }
            };
        }
        if last && !self.held.is_empty() {
            let held = std::mem::take(&mut self.held);
            out.write(part.len(), &held);
        }
        out.done()
    }

    fn step(&mut self, targets: &[Target], byte: u8, at: usize, out: &mut Output) {
        match self.read {
            Read::Over { .. } | Read::Text { .. } => {}
            Read::Key { escaped } => self.key_byte(targets, byte, at, out, escaped),
            Read::Literal if is_literal(byte) => self.emit(byte, at, out),
            Read::Literal => {
                self.value_ended();
                self.between(targets, byte, at, out);
            }
            Read::Between => self.between(targets, byte, at, out),
        }
    }

    /// A byte passed on, held back, or dropped
    fn emit(&mut self, byte: u8, at: usize, out: &mut Output) {
        if self.holding {
            out.end_run(at);
            self.held.push(byte);
        } else if self.dropping {
            out.end_run(at);
        } else {
            out.pass(at);
        }
    }

    fn between(&mut self, targets: &[Target], byte: u8, at: usize, out: &mut Output) {
        if byte.is_ascii_whitespace() {
            return self.emit(byte, at, out);
        }
        match (self.next, byte) {
            (Next::Value, _) => self.value_start(targets, byte, at, out),
            (Next::KeyOrEnd, b'"') => {
                self.holding = true;
                self.key.clear();
                self.emit(byte, at, out);
                self.read = Read::Key { escaped: false };
            }
            (Next::KeyOrEnd | Next::After, b'}' | b']') if !self.holding => {
                self.close(targets, byte, at, out);
            }
            (Next::Colon, b':') => {
                self.emit(byte, at, out);
                self.next = Next::Value;
            }
            (Next::After, b',') => {
                self.holding = true;
                self.emit(byte, at, out);
                let Some(frame) = self.frames.last_mut() else {
                    return self.give_up(at, out);
                };
                frame.index += usize::from(!frame.object);
                self.next = if frame.object {
                    Next::KeyOrEnd
                } else {
                    Next::Value
                };
            }
            _ => self.give_up(at, out),
        }
    }

    /// The first byte of a value: an array's element is kept or not here, a member's value is
    /// replaced here when it's set
    fn value_start(&mut self, targets: &[Target], byte: u8, at: usize, out: &mut Output) {
        if self.frames.last().is_some_and(|frame| !frame.object) {
            self.decide(targets, at, out);
        }
        let setting = self
            .frames
            .last_mut()
            .and_then(|frame| frame.setting.take());
        if let Some(value) = setting.and_then(|index| set_value(targets, index)) {
            out.write(at, value);
            self.dropping = true;
        }
        self.emit(byte, at, out);
        let followed = !self.dropping && matches!(byte, b'{' | b'[') && self.leads_into(targets);
        self.read = match byte {
            b'{' | b'[' if followed => {
                let into = self.targets_into(targets);
                self.frames.push(Frame::new(byte == b'{', into));
                self.next = if byte == b'{' {
                    Next::KeyOrEnd
                } else {
                    Next::Value
                };
                Read::Between
            }
            b'{' | b'[' => Read::Over {
                depth: 1,
                text: false,
                escaped: false,
            },
            b'"' => Read::Text { escaped: false },
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n' => Read::Literal,
            _ => return self.give_up(at, out),
        };
    }

    /// A value ended: what follows is a comma or its container's end
    fn value_ended(&mut self) {
        self.read = Read::Between;
        self.dropping = false;
        self.next = if self.frames.is_empty() {
            Next::Done
        } else {
            Next::After
        };
    }

    /// Bytes of an object or array not followed, from `at`: passed on (or dropped) in one run,
    /// to its end or the part's. Returns where reading goes on.
    fn over_run(&mut self, part: &[u8], mut at: usize, out: &mut Output) -> usize {
        let Read::Over {
            mut depth,
            mut text,
            mut escaped,
        } = self.read
        else {
            return at;
        };
        self.run_from(at, out);
        // One loop over the bytes, which only stops at quotes, backslashes and brackets
        while let Some(byte) = part.get(at).copied() {
            at += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if !special(byte) {
                continue;
            }
            match (text, byte) {
                (true, b'\\') => escaped = true,
                (true, b'"') => text = false,
                (false, b'"') => text = true,
                (false, b'{' | b'[') => depth += 1,
                (false, b'}' | b']') => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                self.value_ended();
                return at;
            }
        }
        self.read = Read::Over {
            depth,
            text,
            escaped,
        };
        at
    }

    /// Bytes of a string value, from `at`: passed on (or dropped) in one run, to its closing
    /// quote or the part's end. Returns where reading goes on.
    fn text_run(&mut self, part: &[u8], at: usize, out: &mut Output) -> usize {
        let Read::Text { escaped } = self.read else {
            return at;
        };
        self.run_from(at, out);
        let (next, text, escaped) = string_run(part, at, escaped);
        if text {
            self.read = Read::Text { escaped };
        } else {
            self.value_ended();
        }
        next
    }

    /// A run of a value's bytes starts at `at`: passed on, or dropped
    fn run_from(&mut self, at: usize, out: &mut Output) {
        if self.dropping {
            out.end_run(at);
        } else {
            out.pass(at);
        }
    }

    fn key_byte(
        &mut self,
        targets: &[Target],
        byte: u8,
        at: usize,
        out: &mut Output,
        escaped: bool,
    ) {
        self.emit(byte, at, out);
        if escaped || byte != b'"' {
            self.key.push(byte);
            self.read = Read::Key {
                escaped: !escaped && byte == b'\\',
            };
            return;
        }
        // The key is whole: its member is kept, set or removed
        self.read = Read::Between;
        self.next = Next::Colon;
        self.decide(targets, at + 1, out);
    }

    /// Whether the current member or element is kept, replaced, or removed: what's held is
    /// written (with a comma after a kept one) or dropped
    fn decide(&mut self, targets: &[Target], at: usize, out: &mut Output) {
        let target = self.target_here(targets);
        let held = std::mem::take(&mut self.held);
        self.holding = false;
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        if let Some(index) = target {
            frame.found.push(index);
        }
        if target
            .and_then(|index| targets.get(index))
            .is_some_and(Target::removes)
        {
            self.dropping = true;
            return;
        }
        frame.setting = target.filter(|index| set_value(targets, *index).is_some());
        let comma = held.first() == Some(&b',');
        let skip = usize::from(comma && frame.kept == 0);
        if frame.kept > 0 && !comma {
            out.write(at, b",");
        }
        out.write(at, held.get(skip..).unwrap_or_default());
        frame.kept += 1;
    }

    /// `}` or `]` of a followed container: missing members set are added before it
    fn close(&mut self, targets: &[Target], byte: u8, at: usize, out: &mut Output) {
        let Some(frame) = self.frames.pop() else {
            return self.give_up(at, out);
        };
        if frame.object != (byte == b'}') {
            return self.give_up(at, out);
        }
        let depth = self.frames.len() + 1;
        let mut kept = frame.kept;
        for index in &frame.targets {
            let missing = !frame.found.contains(index);
            let Some(member) = targets
                .get(*index)
                .filter(|_| missing)
                .and_then(|target| target.member(depth))
            else {
                continue;
            };
            out.write(at, if kept > 0 { b"," } else { b"" });
            out.write(at, &member);
            kept += 1;
        }
        out.pass(at);
        self.read = Read::Between;
        self.value_ended();
    }

    /// Not JSON from here: what's held, then the rest of the body, pass as they come
    fn give_up(&mut self, at: usize, out: &mut Output) {
        let held = std::mem::take(&mut self.held);
        out.write(at, &held);
        out.pass(at);
        self.passing = true;
    }

    fn leads_into(&self, targets: &[Target]) -> bool {
        !self.targets_into(targets).is_empty()
    }

    /// The targets going deeper than the value starting here
    fn targets_into(&self, targets: &[Target]) -> Vec<usize> {
        let depth = self.frames.len();
        let candidates: Vec<usize> = match self.frames.last() {
            None => (0..targets.len()).collect(),
            Some(frame) => frame.targets.clone(),
        };
        candidates
            .into_iter()
            .filter(|index| {
                targets.get(*index).is_some_and(|target| {
                    target.steps.len() > depth
                        && (depth == 0 || self.at_step(target.steps.get(depth - 1)))
                })
            })
            .collect()
    }

    /// The target ending at the current member or element
    fn target_here(&self, targets: &[Target]) -> Option<usize> {
        let frame = self.frames.last()?;
        let depth = self.frames.len();
        frame.targets.iter().copied().find(|index| {
            targets.get(*index).is_some_and(|target| {
                target.steps.len() == depth && self.at_step(target.steps.last())
            })
        })
    }

    /// Whether the current member or element is at `step`
    fn at_step(&self, step: Option<&Step>) -> bool {
        let Some(frame) = self.frames.last() else {
            return false;
        };
        match step {
            Some(Step::Key(key)) => frame.object && key_is(&self.key, key),
            Some(Step::Index(index)) => !frame.object && frame.index == *index,
            None => false,
        }
    }
}

impl Frame {
    fn new(object: bool, targets: Vec<usize>) -> Self {
        Self {
            object,
            targets,
            index: 0,
            kept: 0,
            setting: None,
            found: vec![],
        }
    }
}

impl Target {
    fn removes(&self) -> bool {
        matches!(self.action, Action::Remove)
    }

    /// `"key":value`, for a member set but missing from the object at `depth`
    fn member(&self, depth: usize) -> Option<Vec<u8>> {
        let (Action::Set(value), Some(key)) = (&self.action, &self.key) else {
            return None;
        };
        if self.steps.len() != depth {
            return None;
        }
        let mut member = Vec::with_capacity(key.len() + 1 + value.len());
        member.extend_from_slice(key);
        member.push(b':');
        member.extend_from_slice(value);
        Some(member)
    }
}

fn set_value(targets: &[Target], index: usize) -> Option<&[u8]> {
    match &targets.get(index)?.action {
        Action::Set(value) => Some(value),
        Action::Remove => None,
    }
}

/// Whether a key, as written in the body, is `name` (unescaped only when it has escapes)
fn key_is(written: &[u8], name: &str) -> bool {
    if !written.contains(&b'\\') {
        return written == name.as_bytes();
    }
    let mut quoted = Vec::with_capacity(written.len() + 2);
    quoted.push(b'"');
    quoted.extend_from_slice(written);
    quoted.push(b'"');
    serde_json::from_slice::<String>(&quoted).is_ok_and(|key| key == name)
}

/// Bytes that matter in an object or array not followed: quotes, backslashes and brackets. A
/// table: about twice as fast as comparing each byte with them.
// Its indexes are bytes, below its length
#[allow(clippy::indexing_slicing)]
static SPECIAL: [bool; 256] = {
    let mut table = [false; 256];
    table[b'"' as usize] = true;
    table[b'\\' as usize] = true;
    table[b'{' as usize] = true;
    table[b'}' as usize] = true;
    table[b'[' as usize] = true;
    table[b']' as usize] = true;
    table
};

// A byte is below the table's length: this can't panic, and `get` would cost a check per byte
#[allow(clippy::indexing_slicing)]
fn special(byte: u8) -> bool {
    SPECIAL[usize::from(byte)]
}

/// Reads a string from `at` (after its opening quote), to its closing quote or the part's end:
/// where reading goes on, whether still in the string, and whether the last byte was `\`
fn string_run(part: &[u8], mut at: usize, escaped: bool) -> (usize, bool, bool) {
    if escaped {
        // The escaped byte, whatever it is (in a later part, if this one has no more)
        if at >= part.len() {
            return (at, true, true);
        }
        at += 1;
    }
    while let Some(next) = part
        .get(at..)
        .and_then(|rest| memchr::memchr2(b'"', b'\\', rest))
    {
        at += next;
        if part.get(at) == Some(&b'"') {
            return (at + 1, false, false);
        }
        // `\` and the byte it escapes, which may be in the next part
        if at + 1 >= part.len() {
            return (part.len(), true, true);
        }
        at += 2;
    }
    (part.len().max(at), true, false)
}

fn is_literal(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'+' | b'.')
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{Action, Editor, Target};
    use crate::body_rewrite::path;

    fn set(path: &str, value: serde_json::Value) -> Target {
        let steps = path::parse(path).unwrap();
        let key = match steps.last() {
            Some(path::Step::Key(key)) => Some(serde_json::to_vec(key).unwrap()),
            _ => None,
        };
        Target {
            steps,
            action: Action::Set(serde_json::to_vec(&value).unwrap()),
            key,
        }
    }

    fn remove(path: &str) -> Target {
        Target {
            steps: path::parse(path).unwrap(),
            action: Action::Remove,
            key: None,
        }
    }

    /// The body edited whole, after checking it's edited the same however it's cut in two or
    /// three parts
    fn edit(targets: &[Target], body: &str) -> String {
        let whole = edit_parts(targets, &[body.as_bytes()]);
        let bytes = body.as_bytes();
        for cut in 0..=bytes.len() {
            let (a, b) = bytes.split_at(cut);
            assert_eq!(edit_parts(targets, &[a, b]), whole, "cut at {cut}: {body}");
            for second in cut..=bytes.len() {
                let (b, c) = bytes.split_at(second);
                let a = &b[..cut];
                let b = &b[cut..];
                assert_eq!(
                    edit_parts(targets, &[a, b, c]),
                    whole,
                    "cut at {cut}, {second}: {body}"
                );
            }
        }
        whole
    }

    fn edit_parts(targets: &[Target], parts: &[&[u8]]) -> String {
        let mut editor = Editor::default();
        let mut out = vec![];
        for (index, part) in parts.iter().enumerate() {
            let part = Bytes::copy_from_slice(part);
            out.extend_from_slice(&editor.edit(targets, &part, index + 1 == parts.len()));
        }
        String::from_utf8(out).unwrap()
    }

    const BODY: &str = r#"{"user": {"id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#;

    #[test]
    fn test_setting_values() {
        assert_eq!(
            edit(&[set("$.user.role", "guest".into())], BODY),
            r#"{"user": {"id": 42, "role": "guest"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#
        );
        assert_eq!(
            edit(
                &[set("$.items[1].id", serde_json::json!({ "a": [1, "}"] }))],
                BODY
            ),
            r#"{"user": {"id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": {"a":[1,"}"]}}, {"id": 3}]}"#
        );
        assert_eq!(edit(&[set("$[1]", 9.into())], "[1, 2, 3]"), "[1, 9, 3]");
        assert_eq!(
            edit(
                &[set("$.a", "x".into())],
                r#"{"a": {"deep": [1, {"b": "]"}]}}"#
            ),
            r#"{"a": "x"}"#
        );
    }

    #[test]
    fn test_setting_members_that_arent_there() {
        assert_eq!(
            edit(&[set("$.user.plan", "free".into())], BODY),
            r#"{"user": {"id": 42, "role": "admin","plan":"free"}, "debug": true, "items": [{"id": 1}, {"id": 2}, {"id": 3}]}"#
        );
        assert_eq!(edit(&[set("$.a", 1.into())], " { } "), r#" { "a":1} "#);
        assert_eq!(
            edit(&[set("$['a \"b\"']", 1.into())], "{}"),
            r#"{"a \"b\"":1}"#
        );
        // Not in an object that isn't there, nor past an array's end, nor in a value
        assert_eq!(edit(&[set("$.missing.a", 1.into())], BODY), BODY);
        assert_eq!(edit(&[set("$.items[9]", 1.into())], BODY), BODY);
        assert_eq!(edit(&[set("$.debug.a", 1.into())], BODY), BODY);
    }

    #[test]
    fn test_removing_members() {
        assert_eq!(
            edit(&[remove("$.user")], r#"{"user": 1, "b": 2}"#),
            r#"{ "b": 2}"#
        );
        assert_eq!(
            edit(&[remove("$.b")], r#"{"a": 1, "b": 2, "c": 3}"#),
            r#"{"a": 1, "c": 3}"#
        );
        assert_eq!(edit(&[remove("$.c")], r#"{"a": 1, "c": 3}"#), r#"{"a": 1}"#);
        assert_eq!(edit(&[remove("$.a")], r#"{ "a" : 1 }"#), "{  }");
        assert_eq!(
            edit(
                &[remove("$.a"), remove("$.b")],
                r#"{"a": 1, "b": 2, "c": 3}"#
            ),
            r#"{ "c": 3}"#
        );
        assert_eq!(
            edit(&[remove("$['a\"b']")], r#"{"a\"b": 1, "c": 2}"#),
            r#"{ "c": 2}"#
        );
        assert_eq!(edit(&[remove("$.missing")], BODY), BODY);
    }

    #[test]
    fn test_removing_elements() {
        assert_eq!(edit(&[remove("$[0]")], "[1, 2, 3]"), "[ 2, 3]");
        assert_eq!(edit(&[remove("$[1]")], "[1, 2, 3]"), "[1, 3]");
        assert_eq!(edit(&[remove("$[2]")], "[1, 2, 3]"), "[1, 2]");
        assert_eq!(edit(&[remove("$[0]")], "[ 1 ]"), "[  ]");
        assert_eq!(edit(&[remove("$[3]")], "[1, 2, 3]"), "[1, 2, 3]");
        assert_eq!(
            edit(&[remove("$.items[1]")], BODY),
            r#"{"user": {"id": 42, "role": "admin"}, "debug": true, "items": [{"id": 1}, {"id": 3}]}"#
        );
    }

    #[test]
    fn test_several_changes_and_strings_with_brackets() {
        let body = r#"{"note": "a {tricky} \"string\" ]", "user": {"role": "admin", "x": [1, "}"]}, "debug": true}"#;
        let edited = edit(
            &[
                set("$.user.role", "guest".into()),
                remove("$.debug"),
                set("$.source", "edge".into()),
            ],
            body,
        );
        let parsed: serde_json::Value = serde_json::from_str(&edited).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({ "note": "a {tricky} \"string\" ]", "user": { "role": "guest", "x": [1, "}"] }, "source": "edge" })
        );
    }

    #[test]
    fn test_bodies_that_arent_json_pass_as_they_come() {
        for body in [
            "",
            "not json",
            "[1, 2",
            r#"{"user": "#,
            "{,}",
            "123",
            r#""text""#,
        ] {
            assert_eq!(
                edit(&[set("$.user", 1.into()), remove("$.a")], body),
                body,
                "{body}"
            );
        }
        // What was changed before a body stops being JSON stays changed: it was sent
        assert_eq!(edit(&[remove("$.a")], r#"{"a": 1,}"#), "{,}");
    }
}
