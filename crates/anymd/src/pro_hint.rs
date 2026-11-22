//! Optional CLI discovery. Never called by MCP; this path owns no command output.

use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use anymd::{document::EvidenceKind, pro};

const MARKER: &str = ".pro-hint-shown";

pub fn after_success(kind: EvidenceKind) {
    let stderr = io::stderr();
    if !stderr.is_terminal() {
        return;
    }
    let disabled = std::env::var_os("ANYMD_NO_PRO_HINT").as_deref() == Some(OsStr::new("1"))
        || std::env::var_os("NO_PRO_HINT").is_some()
        || std::env::var_os("CI").is_some();
    if disabled {
        return;
    }
    show_once(
        kind,
        || pro::current_license().is_some(),
        || pro::token_path().and_then(|path| path.parent().map(std::path::Path::to_path_buf)),
        stderr.lock(),
    );
}

fn show_once(
    kind: EvidenceKind,
    pro_active: impl FnOnce() -> bool,
    state_dir: impl FnOnce() -> Option<PathBuf>,
    mut stderr: impl Write,
) {
    let Some(root) = state_dir() else { return };
    let path = root.join(MARKER);
    if path.exists() || pro_active() || std::fs::create_dir_all(root).is_err() {
        return;
    }
    // Exclusive creation enforces one hint across concurrent processes. If
    // persistence or writing stderr fails, leave the command result alone.
    if std::fs::File::create_new(path).is_err() {
        return;
    }
    let (feature, surface) = match kind {
        EvidenceKind::MultiPagePdf => ("verify PDF quotes", "cli_pdf"),
        EvidenceKind::Video => ("extract verifiable video frames", "cli_video"),
    };
    let _ = writeln!(
        stderr,
        "anymd Pro: {feature}. Run `anymd pro buy` or visit {}?entry_surface={surface}",
        pro::PRO_URL
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_entitlement_does_not_consume_the_hint() {
        let state = tempfile::tempdir().unwrap();
        let mut stderr = Vec::new();
        let dir = || Some(state.path().to_path_buf());
        show_once(EvidenceKind::MultiPagePdf, || true, dir, &mut stderr);
        assert!(stderr.is_empty());
        assert!(!state.path().join(MARKER).exists());
        show_once(EvidenceKind::MultiPagePdf, || false, dir, &mut stderr);
        assert!(String::from_utf8(stderr)
            .unwrap()
            .contains("entry_surface=cli_pdf"));
        assert!(state.path().join(MARKER).is_file());
    }

    #[test]
    fn unavailable_state_or_broken_stderr_is_non_fatal() {
        let state = tempfile::tempdir().unwrap();
        let occupied = state.path().join("occupied");
        std::fs::write(&occupied, b"not a directory").unwrap();
        let mut stderr = Vec::new();
        show_once(EvidenceKind::Video, || false, || None, &mut stderr);
        show_once(
            EvidenceKind::Video,
            || false,
            || Some(occupied),
            &mut stderr,
        );
        assert!(stderr.is_empty());

        struct BrokenStderr;
        impl Write for BrokenStderr {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        show_once(
            EvidenceKind::Video,
            || false,
            || Some(state.path().to_path_buf()),
            BrokenStderr,
        );
        assert!(state.path().join(MARKER).is_file());
    }
}
