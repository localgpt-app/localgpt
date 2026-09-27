//! What this machine can run, for picking a local model without asking the
//! user to know their own RAM.
//!
//! Total physical memory only — that's the constraint that actually decides
//! whether a GGUF loads next to Bevy's renderer. Deliberately dependency-free
//! (one `sysctl` / `/proc/meminfo` / `GlobalMemoryStatusEx` read) rather than
//! pulling in a system-info crate for a single number.

/// Total physical RAM in GiB, or `None` when it can't be read.
pub fn total_ram_gb() -> Option<u32> {
    total_ram_bytes().map(|bytes| (bytes / (1024 * 1024 * 1024)) as u32)
}

#[cfg(target_os = "macos")]
fn total_ram_bytes() -> Option<u64> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn total_ram_bytes() -> Option<u64> {
    // MemTotal is in kB.
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn total_ram_bytes() -> Option<u64> {
    None
}

/// Whether this build can put a local model on the GPU. On macOS the
/// `local-llm-metal` feature is what makes a multi-GB GGUF viable beside the
/// renderer — without it, MD and Verse both found the model competing with
/// Bevy for the CPU device map.
pub const fn has_gpu_offload() -> bool {
    cfg!(feature = "local-llm-metal")
}

/// Whether this build can run a GGUF in-process at all.
pub const fn has_local_llm() -> bool {
    cfg!(feature = "local-llm")
}

/// Headroom the renderer and OS need beside the model, in GiB. Verse measured
/// ~7.6 GB free with the window open, and a 5 GB model did not fit the CPU
/// device map there — so a model is only offered when it fits with this much
/// to spare.
const RENDERER_HEADROOM_GB: u32 = 8;

/// Whether a model of `size_gb` is worth offering on this machine.
pub fn fits(size_gb: f32) -> bool {
    match total_ram_gb() {
        // Unknown RAM: don't pretend to know; let the user decide.
        None => true,
        Some(ram) => ram >= size_gb.ceil() as u32 + RENDERER_HEADROOM_GB,
    }
}

/// One line for the model menu, e.g. "32 GB, Metal offload".
pub fn summary() -> String {
    let ram = match total_ram_gb() {
        Some(gb) => format!("{gb} GB RAM"),
        None => "unknown RAM".to_string(),
    };
    let offload = if has_gpu_offload() {
        "Metal offload"
    } else if has_local_llm() {
        "CPU only"
    } else {
        "no in-process LLM in this build"
    };
    format!("{ram}, {offload}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ram_on_supported_platforms() {
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let ram = total_ram_gb().expect("should read RAM on this platform");
            // Any real machine running the test suite has at least 1 GB and
            // less than 8 TB; this catches unit mix-ups (bytes vs kB).
            assert!((1..8192).contains(&ram), "implausible RAM: {ram} GB");
        }
    }

    #[test]
    fn fits_leaves_room_for_the_renderer() {
        // A model as large as all of RAM never fits; a tiny one always does.
        if let Some(ram) = total_ram_gb() {
            assert!(!fits(ram as f32));
            assert!(fits(0.5));
        }
    }

    #[test]
    fn summary_mentions_ram_and_offload() {
        let summary = summary();
        assert!(summary.contains("RAM"), "{summary}");
        assert!(summary.contains(','), "{summary}");
    }
}
