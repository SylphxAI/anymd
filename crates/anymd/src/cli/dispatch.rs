use crate::document::EvidenceKind;
use crate::source_access::SourceAccessPolicy;

use super::{args, cite_check, convert, discovery, output, USAGE};

enum Command {
    Convert,
    Outline,
    Search,
    CiteCheck,
}

pub(super) fn run(
    arguments: &[String],
    policy: &SourceAccessPolicy,
    on_evidence: impl FnOnce(EvidenceKind),
) -> i32 {
    let options = || arguments.iter().take_while(|a| a.as_str() != "--");
    if options().any(|a| a == "-h" || a == "--help") || arguments.is_empty() {
        println!("{USAGE}");
        return if arguments.is_empty() { 2 } else { 0 };
    }
    if options().any(|a| a == "-V" || a == "--version") {
        println!("anymd {}", crate::SERVER_VERSION);
        return 0;
    }
    let (command, options) = match arguments {
        [name, rest @ ..] if name == "outline" => (Command::Outline, rest),
        [name, rest @ ..] if name == "search" => (Command::Search, rest),
        [name, rest @ ..] if matches!(name.as_str(), "cite-check" | "cite_check") => {
            (Command::CiteCheck, rest)
        }
        _ => (Command::Convert, arguments),
    };
    let parsed = match args::parse(options) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("anymd: {message}\nRun `anymd --help` for usage.");
            return 2;
        }
    };
    let result = match command {
        Command::Convert => convert::run(&parsed, policy),
        Command::Outline => discovery::outline(&parsed, policy),
        Command::Search => discovery::search(&parsed, policy),
        Command::CiteCheck => cite_check::run(&parsed, policy),
    };
    let result = match result {
        Ok(result) => result,
        Err(code) => return code,
    };
    if let Err(message) = output::write(&result.text, result.failed, parsed.output.as_deref()) {
        eprintln!("anymd: {message}");
        return 1;
    }
    if let Some(kind) = result.evidence.filter(|_| !result.failed) {
        on_evidence(kind);
    }
    i32::from(result.failed)
}
