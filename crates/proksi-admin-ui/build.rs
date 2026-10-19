//! Embeds `dist/` (the built UI, see README.md): each file with its compressed versions
//! (`.br`, `.gz`, written by the UI's build), or a page saying how to build it

use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

fn files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let dist = root.join("dist");
    println!("cargo:rerun-if-changed={}", dist.to_string_lossy());

    let mut found = vec![];
    files(&dist, &mut found);
    found.sort();

    let mut out = String::from("static ASSETS: &[Asset] = &[\n");
    for file in &found {
        let name = file.to_string_lossy();
        if name.ends_with(".br") || name.ends_with(".gz") {
            continue;
        }
        let Ok(relative) = file.strip_prefix(&dist) else {
            continue;
        };
        let path = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
        let variant = |extension: &str| {
            let compressed = PathBuf::from(format!("{name}.{extension}"));
            if compressed.exists() {
                format!("Some(include_bytes!({:?}))", compressed.to_string_lossy())
            } else {
                "None".to_string()
            }
        };
        let _ = writeln!(
            out,
            "    Asset {{ path: {path:?}, content_type: {:?}, body: include_bytes!({:?}), brotli: {}, gzip: {} }},",
            content_type(&path),
            file.to_string_lossy(),
            variant("br"),
            variant("gz"),
        );
    }
    out.push_str("];\n");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    if let Err(err) = std::fs::write(out_dir.join("assets.rs"), out) {
        println!("cargo:warning=can't write assets.rs: {err}");
        std::process::exit(1);
    }
}
