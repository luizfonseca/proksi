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
/// directory they name. Each segment is looked at once (`lstat`): only symlinks are resolved
/// in full, to see where they lead.
pub fn find(directory: &Path, segments: &[String], index: &[String]) -> Found {
    let mut path = directory.to_path_buf();
    let mut file_type = None;
    for segment in segments {
        path.push(segment);
        let Some((resolved, kind)) = entry(directory, path) else {
            return Found::Missing;
        };
        (path, file_type) = (resolved, Some(kind));
    }
    // No segments: the directory itself
    let file_type = match file_type {
        Some(file_type) => file_type,
        None => match std::fs::metadata(&path) {
            Ok(metadata) => metadata.file_type(),
            Err(_) => return Found::Missing,
        },
    };
    if file_type.is_file() {
        return Found::File(path);
    }
    if file_type.is_dir() {
        return index_file(directory, &path, index);
    }
    Found::Missing
}

/// The first of `index` that's a file in the directory at `path`
fn index_file(directory: &Path, path: &Path, index: &[String]) -> Found {
    index
        .iter()
        .find_map(|name| {
            entry(directory, path.join(name))
                .filter(|(_, kind)| kind.is_file())
                .map(|(file, _)| file)
        })
        .map_or(Found::Missing, Found::File)
}

/// `path` and what it is, a symlink resolved, if it's in `directory`
fn entry(directory: &Path, path: PathBuf) -> Option<(PathBuf, std::fs::FileType)> {
    let file_type = std::fs::symlink_metadata(&path).ok()?.file_type();
    if !file_type.is_symlink() {
        return Some((path, file_type));
    }
    // A symlink inside may lead out of it
    let path = path.canonicalize().ok()?;
    if !path.starts_with(directory) {
        return None;
    }
    let file_type = std::fs::metadata(&path).ok()?.file_type();
    Some((path, file_type))
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

        // Directories too: the files below one leading out aren't served
        fs::create_dir_all(directory.join("docs")).unwrap();
        fs::write(directory.join("docs/a.txt"), "a").unwrap();
        std::os::unix::fs::symlink(root.path(), directory.join("up")).unwrap();
        std::os::unix::fs::symlink(directory.join("docs"), directory.join("guides")).unwrap();
        assert_eq!(
            find(&directory, &["up".into(), "secret.txt".into()], &[]),
            Found::Missing
        );
        assert_eq!(
            find(&directory, &["guides".into(), "a.txt".into()], &[]),
            Found::File(directory.join("docs/a.txt"))
        );
        // Not a directory
        assert_eq!(
            find(&directory, &["real.txt".into(), "x".into()], &[]),
            Found::Missing
        );
    }
}
