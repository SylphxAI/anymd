use pdf_reader_mcp_server::{
    cli, discover_compat, http_transport, source_access::SourceAccessPolicy, PdfReaderMcp,
    SERVER_VERSION,
};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ServerHandler, ServiceExt};

fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match cli::mode(&arguments) {
        cli::Mode::Doctor => {
            doctor();
            Ok(())
        }
        cli::Mode::Cli(arguments) => {
            let policy = SourceAccessPolicy::from_process().map_err(anyhow::Error::msg)?;
            std::process::exit(cli::run(arguments, &policy));
        }
        cli::Mode::Mcp => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(serve()),
    }
}

fn doctor() {
    let tool = |name: &str| {
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
            .unwrap_or(false)
    };
    println!("anymd {SERVER_VERSION} (native Rust)");
    for (name, purpose) in [
        ("tesseract", "OCR for images and scanned PDF pages"),
        ("ffprobe", "audio/video metadata and chapters"),
        ("ffmpeg", "embedded subtitles and transcript audio"),
    ] {
        let state = if tool(name) { "found" } else { "not found" };
        println!("  {name:<12} {state:<10} {purpose}");
    }
    println!("Transcripts (--transcript):");
    for (name, state) in anymd_formats::whisper::status_lines() {
        println!("  {name:<14} {state}");
    }
}

async fn serve() -> anyhow::Result<()> {
    let source_access = SourceAccessPolicy::from_process().map_err(anyhow::Error::msg)?;
    if source_access.is_restricted() {
        eprintln!(
            "[anymd] Filesystem allowlist enabled for {} root(s)",
            source_access.allowed_dir_count()
        );
    }

    if http_transport::transport_from_env().is_some() {
        return http_transport::serve_http(http_transport::HttpConfig::from_env(), source_access)
            .await;
    }

    let server = PdfReaderMcp::with_source_access(source_access);
    let discover_payload = discover_compat::discover_result_value(&server.get_info());
    let (stdin, stdout) = rmcp::transport::stdio();
    let transport = discover_compat::DiscoverAwareTransport::new(
        AsyncRwTransport::new_server(stdin, stdout),
        discover_payload,
    );
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
