use std::path::Path;
use std::time::{Duration, Instant};

use rmcp::{model::CallToolResult, ErrorData};

use crate::command_provider::{self, CommandInvocation};
use crate::schema::InspectArgs;
use crate::source_access::SourceAccessPolicy;

use super::{
    evaluation,
    request::{normalize_args, validate},
    response, source,
};

#[derive(serde::Serialize, serde::Deserialize)]
struct WorkerRequest {
    args: InspectArgs,
    ocr_admitted: bool,
}

pub(crate) async fn inspect(
    mut args: InspectArgs,
    policy: SourceAccessPolicy,
) -> Result<CallToolResult, ErrorData> {
    let started = Instant::now();
    normalize_args(&mut args);
    validate(&args).map_err(|e| ErrorData::invalid_params(e, None))?;
    policy
        .admit_evidence_sources(&mut args.sources)
        .map_err(|e| ErrorData::invalid_params(e, None))?;
    // The parent process holds the existing OCR permit across the supervised
    // worker, so independent cite-check workers cannot evade server admission.
    // A busy OCR owner still permits native positives to be checked.
    let permit = if args.ocr.is_some() {
        crate::ocr_evidence::acquire_ocr_permit().ok()
    } else {
        None
    };
    let ocr_admitted = permit.is_some();
    let timeout = u64::from(args.timeout_ms.unwrap_or(60000));
    let deadline = started + Duration::from_millis(timeout);
    // Parent owns all temporary artifacts, even if the supervised worker is killed.
    let directory =
        tempfile::tempdir().map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let request = directory.path().join("request.json");
    std::fs::write(
        &request,
        serde_json::to_vec(&WorkerRequest {
            args: args.clone(),
            ocr_admitted,
        })
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let maximum = args.max_output_chars.unwrap_or(200000) as usize;
    let executable =
        std::env::current_exe().map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    let result = tokio::task::spawn_blocking(move || {
        let _directory = directory;
        let _permit = permit;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        if remaining == 0 {
            return Err("cite_check deadline exhausted.".into());
        }
        command_provider::run_supervised(CommandInvocation {
            command: executable.to_string_lossy().into(),
            args: vec![
                "__cite-check-worker".into(),
                request.to_string_lossy().into(),
            ],
            timeout_ms: remaining,
            max_stdout_bytes: maximum * 4,
            failure_message: "cite_check worker failed (invalid PDF, extraction or output limit)."
                .into(),
            timeout_message: "cite_check deadline exhausted.".into(),
        })
        .map_err(|e| e.message)
    })
    .await
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    response::from_worker(result, &args, maximum)
}

/// Internal entry point only. Supervision is installed before PDF parsing,
/// network fetching, rendering or model execution; native work is cancellable
/// by terminating this existing command-provider process boundary.
pub fn worker(arguments: &[String]) -> Result<(), String> {
    let request = arguments.first().ok_or("Missing cite-check request.")?;
    let timeout = arguments
        .iter()
        .find_map(|a| a.strip_prefix("--supervised="))
        .ok_or("cite-check worker needs supervision.")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    command_provider::supervise_parent(timeout)?;
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let request_options: WorkerRequest =
        serde_json::from_slice(&std::fs::read(request).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut args = request_options.args;
    normalize_args(&mut args);
    validate(&args)?;
    let (snapshot, hash) = source::snapshot(&args.sources[0], Path::new(request))?;
    let output = if args
        .expected_source_sha256
        .as_ref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&hash))
    {
        response::insufficient(
            &args,
            "Expected source SHA-256 does not match admitted PDF.",
            Some(&hash),
        )
    } else {
        evaluation::evaluate(
            &args,
            &snapshot,
            &hash,
            deadline,
            request_options.ocr_admitted,
        )?
    };
    let serialized = response::serialize(output, &args, &hash)?;
    println!("{serialized}");
    Ok(())
}
