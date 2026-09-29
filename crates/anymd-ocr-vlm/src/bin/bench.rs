//! Measure one runtime over the spike page set: sec/page, peak RSS, CER.
//!
//! docvlm-bench --backend llama|candle|none --pages DIR --models DIR --out FILE

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use anymd_ocr_vlm::{cer, peak_rss_mib, DocOcr, PageOptions};
use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
struct Args {
    /// llama | candle | none (none only proves the harness builds; it is the binary-size baseline)
    #[arg(long, default_value = "none")]
    backend: String,
    /// Directory with `pages/` and `manifest.jsonl` (see bench/make_pages.py).
    #[arg(long)]
    pages: PathBuf,
    /// Model directory (layout: see workflow).
    #[arg(long)]
    models: PathBuf,
    #[arg(long)]
    out: PathBuf,
    /// cpu | metal
    #[arg(long, default_value = "cpu")]
    device: String,
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Stop starting new pages after this many minutes; the rest are reported as skipped.
    #[arg(long, default_value_t = 90)]
    budget_minutes: u64,
    #[arg(long, default_value_t = 50)]
    max_pages: usize,
    #[arg(long, default_value_t = 1024)]
    max_new_tokens: usize,
    #[arg(long, default_value_t = 300)]
    page_timeout_secs: u64,
}

#[derive(Deserialize)]
struct Row {
    file: String,
    cat: String,
    lang: String,
    text: String,
}

#[derive(Serialize)]
struct PageRecord {
    file: String,
    cat: String,
    lang: String,
    status: String,
    secs: f64,
    cer: Option<f64>,
    regions: usize,
    truncated: u32,
    ref_chars: usize,
    hyp_chars: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let threads = if args.threads > 0 {
        args.threads
    } else {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
    };
    let manifest = std::fs::read_to_string(args.pages.join("manifest.jsonl")).context("manifest.jsonl")?;
    let mut rows: Vec<Row> = manifest.lines().map(serde_json::from_str).collect::<Result<_, _>>()?;
    // Interleave categories so a time-budget cut still covers every category.
    rows.sort_by_key(|r| r.file.clone());
    let mut buckets: std::collections::BTreeMap<String, Vec<Row>> = Default::default();
    for r in rows {
        buckets.entry(format!("{}-{}", r.cat, r.lang)).or_default().push(r);
    }
    let mut order = Vec::new();
    while buckets.values().any(|b| !b.is_empty()) {
        for b in buckets.values_mut() {
            if !b.is_empty() {
                order.push(b.remove(0));
            }
        }
    }
    order.truncate(args.max_pages);

    let load_started = Instant::now();
    let mut backend = load_backend(&args, threads)?;
    let load_secs = load_started.elapsed().as_secs_f64();
    let rss_after_load = peak_rss_mib();
    eprintln!("loaded {} in {load_secs:.1}s, rss {rss_after_load:.0} MiB, {threads} threads", backend.as_ref().map_or("none", |b| b.name()));

    let options = PageOptions { max_new_tokens: args.max_new_tokens, page_timeout: Duration::from_secs(args.page_timeout_secs) };
    let started = Instant::now();
    let mut records = Vec::new();
    for row in order {
        let Some(backend) = backend.as_mut() else {
            records.push(skipped(&row, "no backend"));
            continue;
        };
        if started.elapsed() > Duration::from_secs(args.budget_minutes * 60) {
            records.push(skipped(&row, "time budget"));
            continue;
        }
        let image = image::open(args.pages.join("pages").join(&row.file))?.to_rgb8();
        let t = Instant::now();
        let record = match backend.recognize_page(&image, &options) {
            Ok(result) => {
                let hyp = result.text();
                let secs = t.elapsed().as_secs_f64();
                if records.len() < 3 {
                    eprintln!("--- {} ({secs:.1}s)\n{}\n---", row.file, hyp.chars().take(400).collect::<String>());
                }
                PageRecord {
                    file: row.file.clone(),
                    cat: row.cat.clone(),
                    lang: row.lang.clone(),
                    status: "ok".into(),
                    secs,
                    cer: Some(cer(&row.text, &hyp)),
                    regions: result.regions.len(),
                    truncated: result.truncated,
                    ref_chars: row.text.chars().count(),
                    hyp_chars: hyp.chars().count(),
                }
            }
            Err(e) => {
                eprintln!("{} failed: {e:#}", row.file);
                PageRecord { status: format!("error: {e:#}"), secs: t.elapsed().as_secs_f64(), ..skipped(&row, "") }
            }
        };
        eprintln!("{} {} {:.1}s cer={:?}", record.file, record.status, record.secs, record.cer);
        records.push(record);
    }

    let done: Vec<&PageRecord> = records.iter().filter(|r| r.status == "ok").collect();
    let mut secs: Vec<f64> = done.iter().map(|r| r.secs).collect();
    secs.sort_by(f64::total_cmp);
    let pct = |p: f64| secs.get(((secs.len() as f64 - 1.0) * p).round() as usize).copied();
    let summary = serde_json::json!({
        "backend": args.backend,
        "device": args.device,
        "threads": threads,
        "load_secs": load_secs,
        "rss_after_load_mib": rss_after_load,
        "peak_rss_mib": peak_rss_mib(),
        "pages_ok": done.len(),
        "pages_total": records.len(),
        "sec_per_page_mean": if secs.is_empty() { None } else { Some(secs.iter().sum::<f64>() / secs.len() as f64) },
        "sec_per_page_median": pct(0.5),
        "sec_per_page_p90": pct(0.9),
        "cer_mean": mean(done.iter().filter_map(|r| r.cer)),
        "by_category": by_group(&done),
        "pages": records,
    });
    std::fs::write(&args.out, serde_json::to_string_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary["by_category"])?);
    Ok(())
}

fn skipped(row: &Row, why: &str) -> PageRecord {
    PageRecord {
        file: row.file.clone(),
        cat: row.cat.clone(),
        lang: row.lang.clone(),
        status: if why.is_empty() { "error".into() } else { format!("skipped: {why}") },
        secs: 0.0,
        cer: None,
        regions: 0,
        truncated: 0,
        ref_chars: row.text.chars().count(),
        hyp_chars: 0,
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = values.collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn by_group(done: &[&PageRecord]) -> serde_json::Value {
    let mut groups: std::collections::BTreeMap<String, Vec<&PageRecord>> = Default::default();
    for r in done {
        groups.entry(r.cat.clone()).or_default().push(r);
        groups.entry(format!("lang:{}", r.lang)).or_default().push(r);
    }
    groups
        .into_iter()
        .map(|(k, v)| {
            (k, serde_json::json!({
                "pages": v.len(),
                "sec_per_page_mean": mean(v.iter().map(|r| r.secs)),
                "cer_mean": mean(v.iter().filter_map(|r| r.cer)),
            }))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[allow(unused_variables)]
fn load_backend(args: &Args, threads: usize) -> Result<Option<Box<dyn DocOcr>>> {
    match args.backend.as_str() {
        "none" => Ok(None),
        #[cfg(feature = "llama")]
        "llama" => {
            let m = &args.models;
            let b = anymd_ocr_vlm::llama_backend::LlamaBackendOcr::load(
                &m.join("PaddleOCR-VL-1.6.Q4_K_M.gguf"),
                &m.join("mmproj-Q8_0.gguf"),
                &m.join("PP-DocLayoutV3.onnx"),
                threads,
                args.device == "metal",
            )?;
            Ok(Some(Box::new(b)))
        }
        #[cfg(feature = "candle")]
        "candle" => {
            let m = &args.models;
            let b = anymd_ocr_vlm::candle_backend::CandleBackend::load(&m.join("PaddleOCR-VL-1.6"), &m.join("PP-DocLayoutV3_safetensors"), &args.device)?;
            Ok(Some(Box::new(b)))
        }
        other => Err(anyhow!("backend {other} is not compiled in")),
    }
}
