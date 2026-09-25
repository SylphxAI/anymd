use pdf_reader_mcp_server::{
    discover_compat, http_transport, source_access::SourceAccessPolicy, PdfReaderMcp,
    SERVER_VERSION,
};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ServerHandler, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "anymd {SERVER_VERSION}\n\n\
Usage: anymd [doctor] [--allow-dir=<path>]...\n\n\
Filesystem access:\n  \
--allow-dir=<path>       Restrict local PDFs to this directory (repeatable)\n  \
MCP_PDF_ALLOWED_DIRS     Platform path-list of allowed directories\n\n\
Without an allowlist, local PDF access is unrestricted within OS permissions."
        );
        return Ok(());
    }

    if arguments.first().map(String::as_str) == Some("doctor") {
        eprintln!(
            "anymd Rust MCP server {SERVER_VERSION} ({})",
            pdf_reader_core::ENGINE_NAME
        );
        eprintln!("runtime: sole-Rust anymd");
        return Ok(());
    }

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
