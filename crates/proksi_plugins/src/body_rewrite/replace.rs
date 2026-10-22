//! A regular expression replaced in a body as it streams through. A match can be cut by the end
//! of a part, so the last `max_match` bytes of each part are held back, and searched again with
//! the next one: matches up to `max_match` bytes long are all found, wherever parts end.

use std::borrow::Cow;

use bytes::Bytes;
use regex::bytes::Regex;

#[derive(Debug)]
pub(super) struct Replace {
    pub(super) regex: Regex,
    /// What replaces a match (`$1`, `$name` are its groups)
    pub(super) with: Vec<u8>,
    /// The longest match looked for across parts
    pub(super) max_match: usize,
}

/// Where one replacement is in one body, kept from a part to the next
#[derive(Debug, Default)]
pub(super) struct Replacing {
    /// The end of the previous part, held back
    held: Vec<u8>,
}

impl Replacing {
    /// The part of the body to send for `part`, `last` when it's the body's end
    pub(super) fn replace(&mut self, replace: &Replace, part: &Bytes, last: bool) -> Bytes {
        let text: Cow<[u8]> = if self.held.is_empty() {
            Cow::Borrowed(part)
        } else {
            let mut text = std::mem::take(&mut self.held);
            text.extend_from_slice(part);
            Cow::Owned(text)
        };
        // Matches starting before `cut` are final; the rest waits for the next part
        let cut = if last {
            text.len()
        } else {
            text.len().saturating_sub(replace.max_match)
        };

        let mut out = Vec::new();
        let mut from = 0;
        for captures in replace.regex.captures_iter(&text) {
            let Some(found) = captures.get(0).filter(|found| found.start() < cut) else {
                break;
            };
            out.extend_from_slice(text.get(from..found.start()).unwrap_or_default());
            captures.expand(&replace.with, &mut out);
            from = found.end();
        }
        let sent = cut.max(from);
        self.held = text.get(sent..).unwrap_or_default().to_vec();

        // No match and nothing held before: the part as it is, less what's held now
        if from == 0 && out.is_empty() && matches!(text, Cow::Borrowed(_)) {
            return part.slice(..sent);
        }
        out.extend_from_slice(text.get(from..sent).unwrap_or_default());
        Bytes::from(out)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use regex::bytes::Regex;

    use super::{Replace, Replacing};

    fn replace(pattern: &str, with: &str, max_match: usize, parts: &[&str]) -> String {
        let replace = Replace {
            regex: Regex::new(pattern).unwrap(),
            with: with.as_bytes().to_vec(),
            max_match,
        };
        let mut replacing = Replacing::default();
        let mut out = vec![];
        for (index, part) in parts.iter().enumerate() {
            let part = Bytes::copy_from_slice(part.as_bytes());
            out.extend_from_slice(&replacing.replace(&replace, &part, index + 1 == parts.len()));
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_matches_cut_by_parts_are_found() {
        let body = "token=secret-12345, again secret-9 and secret-77";
        let whole = replace("secret-([0-9]+)", "[$1]", 16, &[body]);
        assert_eq!(whole, "token=[12345], again [9] and [77]");
        let bytes = body.as_bytes();
        for cut in 0..=bytes.len() {
            let (a, b) = body.split_at(cut);
            assert_eq!(
                replace("secret-([0-9]+)", "[$1]", 16, &[a, b]),
                whole,
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn test_parts_without_matches_pass_as_they_are() {
        assert_eq!(
            replace("x+", "y", 4, &["aaaa", "bbbb", "cccc"]),
            "aaaabbbbcccc"
        );
        assert_eq!(replace("x+", "y", 4, &["aaxx", "xxbb"]), "aaybb");
    }
}
