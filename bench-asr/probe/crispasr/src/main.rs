fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "missing.gguf".into());
    let loaded = crispasr::CrispASR::new(&path);
    println!("crispasr linked; load ok = {}", loaded.is_ok());
}
