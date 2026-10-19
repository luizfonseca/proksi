//! The file a request path names, inside the directory served: never outside it (`..`,
//! symlinks leading out), and never a hidden one (`.env`, `.git`)

use std::path::{Path, PathBuf};

/// What a path names in the directory
#[derive(Debug, PartialEq)]
pub enum Found {
    File(PathBuf),
    /// Nothing to serve (missing, hidden, outside the directory, or a directory without an
    /// index file)
    Missing,
}

/// The segments of `path` (below the location), decoded (`%20`), or `None` if one can't be
/// served: `..`, hidden (`.name`), or with characters no file name has
pub fn segments(path: &str) -> Option<Vec<String>> {
    let mut segments = vec![];
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let decoded = percent_decode(segment)?;
        if decoded.starts_with('.') || decoded.contains(['/', '\\', '\0']) {
            return None;
        }
        segments.push(decoded);
    }
    Some(segments)
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// The file `segments` name in `directory` (its canonical path), or one of `index` in the
/// directory they name
pub fn find(directory: &Path, segments: &[String], index: &[String]) -> Found {
    let mut path = directory.to_path_buf();
    path.extend(segments);
    let Ok(path) = path.canonicalize() else {
        return Found::Missing;
    };
    // A symlink inside may lead out of it
    if !path.starts_with(directory) {
        return Found::Missing;
    }
    if path.is_file() {
        return Found::File(path);
    }
    if path.is_dir() {
        for name in index {
            let file = path.join(name);
            if file.is_file()
                && file
                    .canonicalize()
                    .is_ok_and(|file| file.starts_with(directory))
            {
                return Found::File(file);
            }
        }
    }
    Found::Missing
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{find, segments, Found};

    fn names(path: &str) -> Option<Vec<String>> {
        segments(path)
    }

    #[test]
    fn test_segments() {
        assert_eq!(
            names("/css/app.css"),
            Some(vec!["css".into(), "app.css".into()])
        );
        assert_eq!(
            names("//css///app.css/"),
            Some(vec!["css".into(), "app.css".into()])
        );
        assert_eq!(names("/my%20file.txt"), Some(vec!["my file.txt".into()]));
        assert_eq!(names("/"), Some(vec![]));
        // Out of the directory, hidden, or encoded to look like something else
        for refused in [
            "/../etc/passwd",
            "/a/..",
            "/.env",
            "/.git/config",
            "/%2e%2e/x",
            "/a%2fb",
            "/a%5cb",
            "/a%00",
            "/bad%zz",
            "/%ff",
        ] {
            assert_eq!(names(refused), None, "{refused}");
        }
    }

    #[test]
    fn test_find() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("site");
        fs::create_dir_all(directory.join("docs")).unwrap();
        fs::write(directory.join("app.js"), "js").unwrap();
        fs::write(directory.join("index.html"), "home").unwrap();
        fs::write(root.path().join("secret.txt"), "secret").unwrap();
        let directory = directory.canonicalize().unwrap();
        let index = vec!["index.html".to_string()];
        let find = |path: &str| find(&directory, &segments(path).unwrap(), &index);

        assert_eq!(find("/app.js"), Found::File(directory.join("app.js")));
        assert_eq!(find("/"), Found::File(directory.join("index.html")));
        // A directory without an index file, and a missing file
        assert_eq!(find("/docs"), Found::Missing);
        assert_eq!(find("/nope.js"), Found::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinks_leading_out_are_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("site");
        fs::create_dir_all(&directory).unwrap();
        fs::write(root.path().join("secret.txt"), "secret").unwrap();
        fs::write(directory.join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret.txt"), directory.join("leak.txt"))
            .unwrap();
        std::os::unix::fs::symlink(directory.join("real.txt"), directory.join("alias.txt"))
            .unwrap();
        let directory = directory.canonicalize().unwrap();

        assert_eq!(find(&directory, &["leak.txt".into()], &[]), Found::Missing);
        // Inside the directory, they're fine
        assert_eq!(
            find(&directory, &["alias.txt".into()], &[]),
            Found::File(directory.join("real.txt"))
        );
    }
}
