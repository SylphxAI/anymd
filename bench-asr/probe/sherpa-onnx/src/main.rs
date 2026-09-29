fn main() {
    let recognizer = sherpa_onnx::OfflineRecognizer::create(&sherpa_onnx::OfflineRecognizerConfig::default());
    println!("sherpa-onnx linked; recognizer = {}", recognizer.is_some());
}
