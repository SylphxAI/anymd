use crate::lean::{result_text, search_with_evidence};
use crate::schema::SearchArgs;
use crate::source_access::SourceAccessPolicy;

use super::args::Parsed;
use super::output::CommandOutput;

pub(super) fn outline(parsed: &Parsed, policy: &SourceAccessPolicy) -> Result<CommandOutput, i32> {
    if parsed.inputs.len() != 1 {
        eprintln!("anymd: outline needs one file or URL");
        return Err(2);
    }
    let args = crate::schema::OutlineArgs {
        timeline: None,
        source: parsed.inputs[0].clone(),
        format: parsed.format.clone().or(Some("tree".into())),
        ocr: parsed.ocr,
        images: parsed.images.clone(),
        revisions: parsed.revisions.clone(),
    };
    match crate::outline::render_with_evidence(&args, policy) {
        Ok((text, evidence)) => Ok(CommandOutput {
            text,
            failed: false,
            evidence,
        }),
        Err(message) => {
            eprintln!("anymd: {message}");
            Err(2)
        }
    }
}

pub(super) fn search(parsed: &Parsed, policy: &SourceAccessPolicy) -> Result<CommandOutput, i32> {
    let Some((query, sources)) = parsed.inputs.split_first() else {
        eprintln!("anymd: search needs a query");
        return Err(2);
    };
    let args = SearchArgs {
        query: query.clone(),
        sources: sources.to_vec(),
        mode: parsed.mode.clone(),
        glob: parsed.glob.clone(),
        case_sensitive: Some(parsed.case_sensitive),
        whole_word: Some(parsed.whole_word),
        max_results: parsed.max,
        context_chars: None,
    };
    match search_with_evidence(&args, policy) {
        Ok((result, evidence)) => Ok(CommandOutput {
            text: result_text(&result),
            failed: result.is_error == Some(true),
            evidence,
        }),
        Err(error) => {
            eprintln!("anymd: {}", error.message);
            Err(2)
        }
    }
}
