use std::{fs, path::Path, process::Command};

const MIN_CUDA_COMPUTE_CAP: u32 = 80;

fn parse_compute_cap(value: &str) -> Option<(String, u32)> {
    let value = value.trim().to_ascii_lowercase();
    let value = value
        .strip_prefix("compute_")
        .or_else(|| value.strip_prefix("sm_"))
        .unwrap_or(&value)
        .replace('.', "");
    let digit_count = value.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let (digits, suffix) = value.split_at(digit_count);
    if !matches!(suffix, "" | "a" | "f") {
        return None;
    }
    let mut base = digits.parse::<u32>().ok()?;
    if base < 20 {
        base *= 10;
    }
    Some((format!("{base}{suffix}"), base))
}

fn detect_local_compute_cap() -> Option<u32> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| parse_compute_cap(line).map(|(_, base)| base))
        // PTX compiled for the oldest GPU reported by nvidia-smi remains
        // loadable on newer GPUs in a heterogeneous machine.
        .min()
}

fn cuda_compute_arch() -> String {
    if let Ok(value) = std::env::var("CUDA_COMPUTE_CAP") {
        let (arch, base) = parse_compute_cap(&value).unwrap_or_else(|| {
            panic!(
                "invalid CUDA_COMPUTE_CAP={value:?}; expected values such as 89, 8.9, sm_89, or compute_89"
            )
        });
        assert!(
            base >= MIN_CUDA_COMPUTE_CAP,
            "oar-ocr-vl CUDA kernels require compute capability 8.0 or newer; got CUDA_COMPUTE_CAP={value:?}"
        );
        return format!("compute_{arch}");
    }

    match detect_local_compute_cap() {
        Some(base) if base >= MIN_CUDA_COMPUTE_CAP => format!("compute_{base}"),
        Some(base) => {
            println!(
                "cargo:warning=detected GPU compute capability {base} is below the oar-ocr-vl CUDA kernel minimum; compiling forward-compatible compute_{MIN_CUDA_COMPUTE_CAP} PTX"
            );
            format!("compute_{MIN_CUDA_COMPUTE_CAP}")
        }
        None => {
            println!(
                "cargo:warning=could not detect a CUDA GPU; compiling compute_{MIN_CUDA_COMPUTE_CAP} PTX (set CUDA_COMPUTE_CAP to override for cross/headless builds)"
            );
            format!("compute_{MIN_CUDA_COMPUTE_CAP}")
        }
    }
}

fn collect_cuda_sources(dir: &Path, sources: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to scan CUDA source directory {dir:?}: {error}"))
    {
        let path = entry.expect("failed to read CUDA source entry").path();
        if path.is_dir() {
            collect_cuda_sources(&path, sources);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("cu") {
            sources.push(path);
        }
    }
}

fn validate_cuda_aggregator(sources: &[std::path::PathBuf]) {
    let aggregator_path = Path::new("src/cuda_kernels.cu");
    let aggregator = fs::read_to_string(aggregator_path)
        .unwrap_or_else(|error| panic!("failed to read {aggregator_path:?}: {error}"));
    for source in sources {
        if source == aggregator_path {
            continue;
        }
        let relative = source
            .strip_prefix("src")
            .expect("CUDA sources are collected under src")
            .to_string_lossy()
            .replace('\\', "/");
        let include = format!("#include \"{relative}\"");
        assert!(
            aggregator.lines().any(|line| line.trim() == include),
            "{aggregator_path:?} must include CUDA source {source:?} as {include:?}"
        );
    }
}

fn nvcc_major_version(nvcc: &std::ffi::OsStr) -> Option<u32> {
    let output = Command::new(nvcc).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The last line reads "Cuda compilation tools, release 13.2, V13.2.78".
    let release = stdout
        .lines()
        .find_map(|line| line.split("release ").nth(1).map(str::to_owned))?;
    release
        .split([',', ' '])
        .next()?
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn main() {
    let mut cuda_sources = Vec::new();
    collect_cuda_sources(Path::new("src"), &mut cuda_sources);
    cuda_sources.sort();
    for source in &cuda_sources {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    validate_cuda_aggregator(&cuda_sources);
    println!("cargo:rerun-if-env-changed=CUDA_COMPUTE_CAP");
    println!("cargo:rerun-if-env-changed=NVCC");
    let metal_enabled = std::env::var_os("CARGO_FEATURE_METAL").is_some();
    let cuda_enabled = std::env::var_os("CARGO_FEATURE_CUDA").is_some();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // The build host triple (e.g. x86_64-pc-windows-msvc): -Xcompiler options
    // go to the *host* compiler, so gate MSVC-only flags on HOST, not target.
    let host_is_msvc = std::env::var("HOST")
        .map(|host| host.ends_with("-msvc"))
        .unwrap_or_default();

    if metal_enabled && target_os != "macos" {
        panic!("oar-ocr-vl feature `metal` is only supported on macOS targets");
    }

    if cuda_enabled {
        let cuda_arch = cuda_compute_arch();
        let nvcc = std::env::var_os("NVCC").unwrap_or_else(|| "nvcc".into());
        let out_dir = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo always sets OUT_DIR"),
        );
        let mut command = Command::new(&nvcc);
        command
            .args(["--ptx", "--std=c++17", "-O3"])
            .arg(format!("--gpu-architecture={cuda_arch}"));
        // CUDA 13 CCCL headers fatal out (MSVC C1189) under cl.exe's
        // traditional preprocessor; request the conforming one.
        if host_is_msvc && nvcc_major_version(&nvcc).is_some_and(|major| major >= 13) {
            command.arg("-Xcompiler").arg("/Zc:preprocessor");
        }
        let output = command
            .arg("-o")
            .arg(out_dir.join("oar_vl_kernels.ptx"))
            .arg("src/cuda_kernels.cu")
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "failed to invoke {:?} for oar-ocr-vl CUDA kernels; install the CUDA toolkit or set NVCC to the compiler path: {error}",
                    nvcc
                )
            });
        if !output.status.success() {
            panic!(
                "{:?} failed for oar-ocr-vl CUDA kernels ({cuda_arch}):\n{}",
                nvcc,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
