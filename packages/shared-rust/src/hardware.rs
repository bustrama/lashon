//! Hardware-tier detection.
//!
//! Lashon classifies the host into one of four capability tiers (A–D); the
//! tier picks the default STT / LLM / TTS models encoded in
//! `apps/desktop/src-tauri/tiers.json`. The thresholds are the ones in
//! `docs/tech-stack.md`. Detection runs once at onboarding and the user may
//! override the result — see `docs/adr/0013`.
//!
//! The probing is best-effort: every backend (NVML, sysinfo, the Vulkan
//! loader) can be absent, so each probe degrades to a conservative reading
//! rather than failing. The worst case is Tier D, never an error.

use serde::{Deserialize, Serialize};

const BYTES_PER_GIB: f64 = 1_073_741_824.0;

/// A hardware capability tier. See `docs/tech-stack.md` for the per-tier model
/// map; `tiers.json` encodes the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Studio — a 12 GB+ NVIDIA GPU. Everything runs local at full speed.
    A,
    /// Workstation — a 6–8 GB NVIDIA GPU. Local, with smaller models.
    B,
    /// Office — no CUDA GPU but a Vulkan one (or a capable CPU).
    C,
    /// Minimal — the conservative fallback. Dictation local, the rest cloud.
    D,
}

/// STT device mode — probe the GPU first, fall back to CPU (hardware tiers
/// A/B). The value of `LASHON_STT_DEVICE` for those tiers.
pub const STT_DEVICE_AUTO: &str = "auto";

/// STT device mode — run on the CPU, skipping the CUDA runtime entirely
/// (hardware tiers C/D). The value of `LASHON_STT_DEVICE` for those tiers.
pub const STT_DEVICE_CPU: &str = "cpu";

impl Tier {
    /// The single-letter tier code, as stored in settings and `tiers.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::A => "A",
            Tier::B => "B",
            Tier::C => "C",
            Tier::D => "D",
        }
    }

    /// Parse a single-letter tier code (`"A"`–`"D"`) — the inverse of
    /// `as_str`, and the form stored in settings. Unknown input yields `None`.
    pub fn from_code(code: &str) -> Option<Tier> {
        match code {
            "A" => Some(Tier::A),
            "B" => Some(Tier::B),
            "C" => Some(Tier::C),
            "D" => Some(Tier::D),
            _ => None,
        }
    }

    /// The STT device mode for this tier: GPU-probing `auto` on the CUDA tiers
    /// A/B, CPU-only on C/D. For an auto-detected tier this matches what the
    /// engine would choose anyway; its real effect is an explicit user
    /// override — picking C/D on a GPU machine forces the CPU path
    /// (docs/adr/0014).
    pub fn stt_device(self) -> &'static str {
        match self {
            Tier::A | Tier::B => STT_DEVICE_AUTO,
            Tier::C | Tier::D => STT_DEVICE_CPU,
        }
    }
}

/// The raw capability readings a tier is classified from.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HardwareProbe {
    /// An NVIDIA GPU with a working driver is present — the CUDA path.
    pub cuda: bool,
    /// Total VRAM of the largest NVIDIA GPU, in GiB; `0.0` when there is none.
    pub vram_gb: f64,
    /// Total system RAM, in GiB.
    pub ram_gb: f64,
    /// A Vulkan-capable GPU is present — the AMD / Intel acceleration path.
    pub vulkan: bool,
}

/// The detected tier together with the readings behind it. The frontend shows
/// the readings so the user can sanity-check an override.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HardwareReport {
    pub tier: Tier,
    pub probe: HardwareProbe,
}

/// Classify a probe into a tier. Pure — exactly the thresholds of
/// `docs/tech-stack.md`, in the A→B→C→D order detection tests them.
pub fn classify(p: &HardwareProbe) -> Tier {
    if p.cuda && p.vram_gb >= 12.0 && p.ram_gb >= 24.0 {
        Tier::A
    } else if p.cuda && p.vram_gb >= 6.0 && p.ram_gb >= 12.0 {
        Tier::B
    } else if p.vulkan && p.ram_gb >= 8.0 {
        Tier::C
    } else {
        Tier::D
    }
}

/// Probe the host and classify it. Never fails — see the module docs.
pub fn detect() -> HardwareReport {
    let (cuda, vram_gb) = probe_nvidia();
    let probe = HardwareProbe {
        cuda,
        vram_gb,
        ram_gb: probe_ram_gb(),
        vulkan: probe_vulkan(),
    };
    let tier = classify(&probe);
    tracing::info!(
        tier = tier.as_str(),
        cuda,
        vram_gb,
        ram_gb = probe.ram_gb,
        vulkan = probe.vulkan,
        "hardware tier detected"
    );
    HardwareReport { tier, probe }
}

/// `(an NVIDIA GPU is present, the largest GPU's VRAM in GiB)`. NVML init
/// fails with no NVIDIA driver — that is the common, expected non-NVIDIA case.
fn probe_nvidia() -> (bool, f64) {
    let nvml = match nvml_wrapper::Nvml::init() {
        Ok(nvml) => nvml,
        Err(err) => {
            tracing::debug!("NVML unavailable (no NVIDIA GPU expected): {err}");
            return (false, 0.0);
        }
    };
    let count = nvml.device_count().unwrap_or(0);
    let mut max_vram_gb = 0.0_f64;
    for index in 0..count {
        match nvml
            .device_by_index(index)
            .and_then(|dev| dev.memory_info())
        {
            Ok(mem) => max_vram_gb = max_vram_gb.max(mem.total as f64 / BYTES_PER_GIB),
            Err(err) => tracing::debug!("NVML device {index} unreadable: {err}"),
        }
    }
    (count > 0, max_vram_gb)
}

/// Total system RAM in GiB.
fn probe_ram_gb() -> f64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory() as f64 / BYTES_PER_GIB
}

/// Whether a Vulkan-capable GPU is present — a minimal instance is created and
/// its physical devices enumerated. A software rasterizer (`CPU` device type)
/// does not count: Tier C wants a real GPU.
fn probe_vulkan() -> bool {
    use ash::vk;

    // SAFETY: `Entry::load` dynamically loads the system Vulkan loader; it is
    // unsafe only because it dlopen's a library. A missing loader is an `Err`.
    let entry = match unsafe { ash::Entry::load() } {
        Ok(entry) => entry,
        Err(err) => {
            tracing::debug!("Vulkan loader unavailable: {err}");
            return false;
        }
    };

    let app_info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_0);
    let create_info = vk::InstanceCreateInfo::default().application_info(&app_info);
    // SAFETY: `create_info` is fully initialized and outlives the call; no
    // extensions or layers are requested.
    let instance = match unsafe { entry.create_instance(&create_info, None) } {
        Ok(instance) => instance,
        Err(err) => {
            tracing::debug!("Vulkan instance creation failed: {err}");
            return false;
        }
    };

    // SAFETY: `instance` is live for the whole block and destroyed below.
    let devices = unsafe { instance.enumerate_physical_devices() }.unwrap_or_default();
    let has_gpu = devices.iter().any(|&device| {
        let props = unsafe { instance.get_physical_device_properties(device) };
        matches!(
            props.device_type,
            vk::PhysicalDeviceType::DISCRETE_GPU
                | vk::PhysicalDeviceType::INTEGRATED_GPU
                | vk::PhysicalDeviceType::VIRTUAL_GPU
        )
    });
    // SAFETY: no child objects (devices, surfaces) were created from `instance`.
    unsafe { instance.destroy_instance(None) };
    has_gpu
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(cuda: bool, vram_gb: f64, ram_gb: f64, vulkan: bool) -> HardwareProbe {
        HardwareProbe {
            cuda,
            vram_gb,
            ram_gb,
            vulkan,
        }
    }

    #[test]
    fn a_studio_gpu_classifies_as_tier_a() {
        assert_eq!(classify(&probe(true, 16.0, 32.0, true)), Tier::A);
        // Exactly on the threshold.
        assert_eq!(classify(&probe(true, 12.0, 24.0, false)), Tier::A);
    }

    #[test]
    fn a_mid_range_gpu_classifies_as_tier_b() {
        assert_eq!(classify(&probe(true, 8.0, 16.0, true)), Tier::B);
        assert_eq!(classify(&probe(true, 6.0, 12.0, false)), Tier::B);
    }

    #[test]
    fn a_studio_gpu_starved_of_ram_falls_to_tier_b() {
        // 16 GB VRAM but only 16 GB RAM misses Tier A's 24 GB RAM floor.
        assert_eq!(classify(&probe(true, 16.0, 16.0, true)), Tier::B);
    }

    #[test]
    fn a_vulkan_gpu_without_cuda_classifies_as_tier_c() {
        assert_eq!(classify(&probe(false, 0.0, 16.0, true)), Tier::C);
        assert_eq!(classify(&probe(false, 0.0, 8.0, true)), Tier::C);
    }

    #[test]
    fn an_underpowered_nvidia_gpu_drops_to_the_vulkan_path() {
        // A 4 GB CUDA GPU misses Tier B; its Vulkan support still reaches C.
        assert_eq!(classify(&probe(true, 4.0, 16.0, true)), Tier::C);
    }

    #[test]
    fn no_gpu_or_too_little_ram_classifies_as_tier_d() {
        assert_eq!(classify(&probe(false, 0.0, 16.0, false)), Tier::D);
        // A Vulkan GPU but under the 8 GB RAM floor is still Tier D.
        assert_eq!(classify(&probe(false, 0.0, 4.0, true)), Tier::D);
        assert_eq!(classify(&probe(false, 0.0, 2.0, false)), Tier::D);
    }

    #[test]
    fn tier_codes_are_single_letters() {
        assert_eq!(Tier::A.as_str(), "A");
        assert_eq!(Tier::D.as_str(), "D");
    }

    #[test]
    fn tier_codes_round_trip_through_from_code() {
        for tier in [Tier::A, Tier::B, Tier::C, Tier::D] {
            assert_eq!(Tier::from_code(tier.as_str()), Some(tier));
        }
        assert_eq!(Tier::from_code("E"), None);
        assert_eq!(Tier::from_code(""), None);
    }

    #[test]
    fn gpu_tiers_probe_and_cpu_tiers_force_cpu() {
        assert_eq!(Tier::A.stt_device(), STT_DEVICE_AUTO);
        assert_eq!(Tier::B.stt_device(), STT_DEVICE_AUTO);
        assert_eq!(Tier::C.stt_device(), STT_DEVICE_CPU);
        assert_eq!(Tier::D.stt_device(), STT_DEVICE_CPU);
    }
}
