//! Read actual Linux hardware capabilities even when the assembler has been
//! enabled for FP16. `is_aarch64_feature_detected!` folds to true under +fp16.

pub fn cpu_available() -> bool {
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        // SAFETY: getauxval reads the process's kernel-provided capability word.
        let capabilities = unsafe { libc::getauxval(libc::AT_HWCAP) };
        let required = libc::HWCAP_FPHP | libc::HWCAP_ASIMDHP;
        capabilities & required == required
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "linux")))]
    {
        true
    }
}
