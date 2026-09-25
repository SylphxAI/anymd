// The product version lives in the npm package.json; crate versions do not
// track it. Expose it so the playground shows the anymd release it runs.
fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../package.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let version = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| {
            let start = text.find("\"version\"")?;
            let rest = &text[start + 9..];
            let open = rest.find('"')? + 1;
            let close = rest[open..].find('"')? + open;
            Some(rest[open..close].to_string())
        })
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=ANYMD_VERSION={version}");
}
