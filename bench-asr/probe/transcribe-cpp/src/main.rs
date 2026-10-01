fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "missing.gguf".into());
    let loaded = transcribe_cpp::Model::load(path);
    println!("transcribe-cpp linked; load ok = {}", loaded.is_ok());
}
