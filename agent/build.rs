//! Embeds the Send files web app (`../filesync/web`) into the agent, so the computer can serve it to the
//! local network itself with no separate server. Writes `$OUT_DIR/files_assets.rs`.

use std::{env, fs, path::{Path, PathBuf}};

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let url = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
            // Developer diagnostics are not part of the product.
            if url == "/test.html" || url == "/js/test-diagnostics.js" {
                continue;
            }
            out.push((url, p));
        }
    }
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../filesync/web");
    println!("cargo:rerun-if-changed=../filesync/web");
    println!("cargo:rerun-if-changed=build.rs");
    let mut files = vec![];
    walk(&root, &root, &mut files);
    let mut src = String::from("pub static ASSETS: &[(&str, &str, &[u8])] = &[\n");
    for (url, path) in files {
        let abs = fs::canonicalize(&path).unwrap_or(path);
        let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        src.push_str(&format!("    ({url:?}, {:?}, include_bytes!({abs:?})),\n", mime(&url)));
    }
    src.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("files_assets.rs"), src).unwrap();
}
