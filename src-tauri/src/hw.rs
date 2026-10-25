//! GPU / hardware probing and system metrics.
use crate::base::*;
use std::sync::{LazyLock, Mutex};

// ── GPU helpers ──

/// One NVIDIA card as `nvidia-smi` reports it. `index` is nvidia-smi's
/// enumeration order (PCI order on Windows), `uuid` is the stable identity
/// CUDA itself accepts in `CUDA_VISIBLE_DEVICES`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NvCard {
    pub index: i32,
    pub uuid: String,
    pub name: String,
    pub vram_mb: u64,
    pub free_mb: u64,
    pub driver: String,
}

const NV_CARD_ARGS: [&str; 2] = [
    "--query-gpu=index,uuid,name,memory.total,memory.free,driver_version",
    "--format=csv,noheader,nounits",
];

/// Probe nvidia-smi once for every card, plus the raw text so legacy output
/// (3-field rows, or a test fake) can still fall through to `pick_smi_row`.
fn probe_nv_cards() -> (Vec<NvCard>, String) {
    if let Ok(out) = probe_command("NVIDIA_SMI", "nvidia-smi")
        .args(NV_CARD_ARGS)
        .output()
    {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let cards = parse_nv_cards(&raw);
            if !cards.is_empty() {
                return (cards, raw);
            }
        }
    }
    // A driver too old for `uuid` (or a fake that only answers the classic
    // query) must not turn a working NVIDIA box into "no GPU": ask for the
    // 3-field form and let `pick_smi_row` describe it.
    let raw = probe_command("NVIDIA_SMI", "nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    (Vec::new(), raw)
}

/// Every NVIDIA card, one nvidia-smi probe. Empty on AMD/Intel/CPU boxes.
pub(crate) fn nv_cards() -> Vec<NvCard> {
    probe_nv_cards().0
}

/// Parse `index,uuid,name,memory.total,memory.free,driver_version` rows.
/// Card names may contain commas, so the four trailing scalar fields are read
/// from the right and everything between uuid and them is the name. Pure +
/// unit-tested.
pub(crate) fn parse_nv_cards(text: &str) -> Vec<NvCard> {
    let mut cards = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 6 {
            continue;
        }
        let Ok(index) = parts[0].parse::<i32>() else {
            continue;
        };
        let uuid = parts[1].to_string();
        let driver = parts[parts.len() - 1].to_string();
        let free_mb = parts[parts.len() - 2].parse::<u64>().unwrap_or(0);
        let vram_mb = parts[parts.len() - 3].parse::<u64>().unwrap_or(0);
        let name = parts[2..parts.len() - 3].join(", ");
        if name.is_empty() {
            continue;
        }
        cards.push(NvCard {
            index,
            uuid,
            name,
            vram_mb,
            free_mb,
            driver,
        });
    }
    cards.sort_by_key(|c| c.index);
    cards
}

/// The card WanGP should run on when the user leaves the picker on Auto: the
/// biggest VRAM, then the most free VRAM, then the lowest index so the answer
/// is deterministic on identical cards. Pure + unit-tested.
pub(crate) fn best_nv_card(cards: &[NvCard]) -> Option<&NvCard> {
    cards.iter().max_by(|a, b| {
        a.vram_mb
            .cmp(&b.vram_mb)
            .then(a.free_mb.cmp(&b.free_mb))
            .then(b.index.cmp(&a.index))
    })
}

/// Resolved GPU picker value.
#[derive(Clone, Debug, Default)]
pub(crate) struct GpuPin {
    /// The physical card the pick resolved to, when nvidia-smi reports it.
    pub card: Option<NvCard>,
    /// Value for upstream's `--gpu` flag (`cuda:<nvidia-smi index>`). WanGP
    /// turns it into `CUDA_VISIBLE_DEVICES` itself and, because the flag is
    /// set, calls `torch.set_default_device(...)` after model load — the exact
    /// configuration a manual device pick produces. `None` keeps every GPU
    /// visible and lets WanGP fall back to its own defaults.
    pub arg: Option<String>,
    /// Human-readable warning when the saved pick could not be matched.
    pub note: Option<String>,
}

/// Turn the saved GPU Device pick into an explicit `--gpu cuda:N`.
///
/// Values: `auto` (best card), `uuid:GPU-…` (stable id), `cuda:N` (legacy
/// nvidia-smi index, still honoured), `all` (leave every GPU visible).
/// The launcher also sets `CUDA_DEVICE_ORDER=PCI_BUS_ID`, so the ordinal it
/// passes means the same card nvidia-smi (and the picker) showed.
/// Pure + unit-tested.
pub(crate) fn resolve_gpu_pick(pick: &str, cards: &[NvCard]) -> GpuPin {
    let pick = pick.trim();
    if pick.eq_ignore_ascii_case("all") || pick.eq_ignore_ascii_case("multi") {
        return GpuPin::default();
    }
    let pin_for = |c: &NvCard| GpuPin {
        arg: Some(format!("cuda:{}", c.index)),
        card: Some(c.clone()),
        note: None,
    };
    let auto = |cards: &[NvCard]| best_nv_card(cards).map(pin_for);
    // A UUID card id, optionally prefixed (`uuid:`, `cuda:`, or bare).
    let uuid_pick = pick
        .strip_prefix("uuid:")
        .or_else(|| pick.strip_prefix("cuda:"))
        .filter(|s| s.starts_with("GPU-") || s.starts_with("MIG-"))
        .map(|s| s.trim().to_string())
        .or_else(|| {
            (pick.starts_with("GPU-") || pick.starts_with("MIG-")).then(|| pick.to_string())
        });
    if let Some(uuid) = uuid_pick {
        if uuid.is_empty() {
            return auto(cards).unwrap_or_default();
        }
        if let Some(c) = cards.iter().find(|c| c.uuid.eq_ignore_ascii_case(&uuid)) {
            return pin_for(c);
        }
        // A card the user removed must not pin a ghost and leave torch with
        // zero devices: fall back to Auto and say so.
        if !cards.is_empty() {
            let mut p = auto(cards).unwrap_or_default();
            p.note = Some(format!(
                "saved GPU {uuid} is not installed — using {} instead",
                p.card.as_ref().map(|c| c.name.as_str()).unwrap_or("Auto")
            ));
            return p;
        }
        // No nvidia-smi: there is no ordinal to pass, so leave WanGP alone.
        return GpuPin {
            card: None,
            arg: None,
            note: Some(format!(
                "saved GPU {uuid} could not be verified (nvidia-smi is unavailable) — letting WanGP pick"
            )),
        };
    }
    if cards.is_empty() {
        return GpuPin::default();
    }
    if pick.is_empty() || pick.eq_ignore_ascii_case("auto") {
        return auto(cards).unwrap_or_default();
    }
    let ordinal = pick.strip_prefix("cuda:").unwrap_or(pick);
    if let Ok(n) = ordinal.trim().parse::<i32>() {
        if let Some(c) = cards.iter().find(|c| c.index == n) {
            return pin_for(c);
        }
        // Same fail-safe: a stale ordinal (card removed, eGPU unplugged) must
        // not hide every GPU.
        let mut p = auto(cards).unwrap_or_default();
        p.note = Some(format!(
            "saved pick cuda:{n} is not installed — using {} instead",
            p.card.as_ref().map(|c| c.name.as_str()).unwrap_or("Auto")
        ));
        return p;
    }
    let mut p = auto(cards).unwrap_or_default();
    p.note = Some(format!(
        "unrecognised GPU selection '{pick}' — falling back to Auto"
    ));
    p
}

static GPU_PICK_CACHE: LazyLock<Mutex<(Option<std::time::SystemTime>, String)>> =
    LazyLock::new(|| Mutex::new((None, "auto".to_string())));

/// The saved GPU Device pick (`auto` when unset). Cached on the launcher
/// config file's mtime, so the 2s metrics tick never re-parses JSON and a
/// Save is visible immediately.
pub(crate) fn saved_gpu_pick() -> String {
    let mtime = std::fs::metadata(crate::base::get_config_file())
        .and_then(|m| m.modified())
        .ok();
    if let Ok(g) = GPU_PICK_CACHE.lock() {
        if g.0 == mtime {
            return g.1.clone();
        }
    }
    let pick = crate::base::load_config_value()
        .get("gpuDevice")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .trim()
        .to_string();
    if let Ok(mut g) = GPU_PICK_CACHE.lock() {
        *g = (mtime, pick.clone());
    }
    pick
}

pub(crate) fn get_gpu_info_sync() -> serde_json::Value {
    let (cards, raw) = probe_nv_cards();
    if !cards.is_empty() {
        let pick = saved_gpu_pick();
        let pin = resolve_gpu_pick(&pick, &cards);
        // An unresolved pick (card removed, stale uuid) describes the card
        // that will actually run, never "none".
        if let Some(card) = pin.card.as_ref().or_else(|| best_nv_card(&cards)) {
            return serde_json::json!({
                "vendor":"NVIDIA",
                "name":card.name,
                "vramMB":format!("{} MiB", card.vram_mb),
                "driverVersion":card.driver,
                "uuid":card.uuid,
                "index":card.index,
                "pick":pick,
                "pinNote":pin.note,
                "gpuArg":pin.arg,
                "raw":raw,
            });
        }
    }
    // Legacy 3-field nvidia-smi output (old drivers, test fakes).
    if let Some((name, mb, driver)) = pick_smi_row(&raw) {
        return serde_json::json!({"vendor":"NVIDIA","name":name,"vramMB":format!("{mb} MiB"),"driverVersion":driver,"raw":raw});
    }
    // No NVIDIA driver — WMI fallback for AMD/Intel (Electron queryGpuList parity).
    // Doc-leading: deepbeepmeep docs/AMD-INSTALLATION.md is the spec for AMD support.
    if let Some((name, vendor, _raw_bytes, driver)) = wmi_gpu_fallback() {
        // Prefer 64-bit registry VRAM (AdapterRAM caps ~4GB / often 0).
        match wmi_dedicated_vram_mb(&name) {
            Some(mb) => {
                return serde_json::json!({"vendor":vendor,"name":name,"vramMB":format!("{mb} MiB"),"driverVersion":driver,"raw":format!("WMI+REG: {name}")})
            }
            // Known-card table before giving up (R9700 PRO driver layout
            // reports no parseable registry size).
            None => match known_vram_mb(&name) {
                Some(mb) => {
                    return serde_json::json!({"vendor":vendor,"name":name,"vramMB":format!("{mb} MiB"),"driverVersion":driver,"raw":format!("WMI+TABLE: {name}")})
                }
                None => {
                    return serde_json::json!({"vendor":vendor,"name":name,"vramMB":"0 MiB","driverVersion":driver,"raw":format!("WMI: {name} (VRAM unknown)")})
                }
            },
        }
    }
    serde_json::json!({"vendor":"unknown","name":"","vramMB":"0","driverVersion":"","raw":"nvidia-smi not found"})
}

/// Probe spawn with a hardware-simulation hook for integration tests.
/// `WGP_PROBE_<KIND>` (WGP_PROBE_NVIDIA_SMI / WGP_PROBE_POWERSHELL) redirects
/// the probe through `cmd /C <fake>`; unset in production, where this is
/// exactly silent_command. Lets tests simulate an AMD box (no nvidia-smi,
/// canned WMI/registry answers) on any machine without touching prod behavior.
pub(crate) fn probe_command(kind: &str, bin: &str) -> std::process::Command {
    if let Ok(fake) = std::env::var(format!("WGP_PROBE_{kind}")) {
        let mut c = silent_command("cmd");
        c.args(["/C", &fake, "--"]);
        let _ = bin;
        return c;
    }
    silent_command(bin)
}

/// WMI fallback for non-NVIDIA GPUs on Windows (AMD/Intel).
/// Returns (display name, vendor, AdapterRAM bytes). AdapterRAM is a 32-bit
/// field — capped at ~4GB and frequently 0 — so callers must treat small/zero
/// values as "VRAM unknown", never as truth (mirrors queryGpuList).
/// All non-NVIDIA video controllers: (display name, vendor, AdapterRAM
/// bytes, driver version). One WMI call serves fallback selection,
/// multi-GPU warnings and the driver preflight check. Missing DriverVersion
/// (older query shapes, canned test fakes) parses as "".
#[cfg(windows)]
pub(crate) fn wmi_all_gpus() -> Vec<(String, String, u64, String)> {
    let out = match probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name + '|' + $_.AdapterRAM + '|' + $_.DriverVersion }"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let s = String::from_utf8_lossy(&out.stdout);
    let mut all = Vec::new();
    for ln in s.lines() {
        let mut parts = ln.split('|');
        let (Some(n), Some(r)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = n.trim().to_string();
        if name.is_empty()
            || name.to_lowercase().contains("nvidia")
            || is_virtual_display_adapter(&name)
        {
            continue;
        }
        let lower = name.to_lowercase();
        let vendor = if lower.contains("amd") || lower.contains("radeon") {
            "AMD"
        } else if lower.contains("intel") || lower.contains("arc") {
            "INTEL"
        } else if lower.contains("basic display") {
            // No driver installed: the preflight's fatal check keys on this
            // name, but filtering it out as "unknown vendor" left the check
            // unreachable (everything downstream saw "Unknown").
            "unknown"
        } else {
            continue;
        };
        let raw = r.trim().parse::<u64>().unwrap_or(0);
        let driver = parts.next().unwrap_or("").trim().to_string();
        all.push((name, vendor.to_string(), raw, driver));
    }
    all
}
#[cfg(not(windows))]
pub(crate) fn wmi_all_gpus() -> Vec<(String, String, u64, String)> {
    Vec::new()
}

/// Remote-desktop / VM virtual displays (ToDesk, GameViewer, Parsec, RDP,
/// Hyper-V, VMware, VirtualBox, indirect-display shims) report as video
/// controllers but carry no GPU — never tier on them (#2224: a 7900 XT box
/// was mistiered because a virtual adapter won the pick).
/// "Basic Display Adapter" is NOT virtual here — it's the no-driver state
/// with its own fatal preflight check. Pure + unit-tested.
pub(crate) fn is_virtual_display_adapter(name: &str) -> bool {
    let l = name.to_lowercase();
    [
        "virtual",
        "indirect display",
        "todesk",
        "gameviewer",
        "parsec",
        "remote desktop",
        "remote display",
        "rdp",
        "hyper-v",
        "hyperv",
        "vmware",
        "virtualbox",
        "vbox",
        "qemu",
        "bochs",
        "remotefx",
        "citrix",
        "anydesk",
        "teamviewer",
        "rustdesk",
        "splashtop",
        "moonlight",
        "sunshine",
    ]
    .iter()
    .any(|t| l.contains(t))
}

/// AMD integrated graphics (APU): the iGPU sharing system memory. AMD names
/// these "Radeon Graphics" / "Radeon 780M Graphics" / "Radeon(TM) Graphics",
/// while discrete cards are "Radeon RX ...", "Radeon PRO ...", "Radeon VII",
/// "Radeon Vega ..." (discrete). Used so an iGPU+dGPU box is not counted as
/// two discrete cards. Pure + unit-tested.
pub(crate) fn is_amd_apu(name: &str) -> bool {
    let l = name.to_lowercase();
    // Must look AMD-ish (callers already filter vendor == "AMD", but be safe).
    if !(l.contains("amd") || l.contains("radeon")) {
        return false;
    }
    // Discrete markers win: a card that says RX/PRO/VII/Vega is discrete.
    if l.contains("rx")
        || l.contains("pro")
        || l.contains("vii")
        || l.contains("vega")
        || l.contains("instinct")
    {
        return false;
    }
    // APUs are generically named "Radeon Graphics" / "...M Graphics", or
    // carry a model number ending in 'M' (780M, 680M, 610M, 740M, ...).
    if l.contains("graphics") {
        return true;
    }
    // APU model numbers end in 'M' (780M, 680M, 610M, 740M, ...): a run of
    // digits immediately followed by 'm', with a non-alphanumeric before it.
    let bytes = l.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i] == b'm' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                let prev_ok = j == 0 || !bytes[j - 1].is_ascii_alphanumeric();
                let next_ok = i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_alphanumeric();
                if prev_ok && next_ok {
                    return true;
                }
            }
        }
    }
    false
}

/// Names of virtual display adapters currently visible (Windows WMI).
/// Lets preflight name the adapters it ignored (#2224 visibility).
/// Empty on non-Windows. Uses probe_command so tests can can the answers.
#[cfg(windows)]
pub(crate) fn wmi_virtual_adapters() -> Vec<String> {
    let out = match probe_command("POWERSHELL", "powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter(|n| is_virtual_display_adapter(n))
        .map(str::to_string)
        .collect()
}
#[cfg(not(windows))]
pub(crate) fn wmi_virtual_adapters() -> Vec<String> {
    Vec::new()
}

/// One `nvidia-smi --query-gpu=name,memory.total,driver_version` row:
/// `(name, vramMiB, driver)`. Names may contain commas, so the memory and
/// driver fields are read from the right. Multi-GPU output (one row per card)
/// keeps the largest card — the one that can actually run WanGP. Pure +
/// unit-tested.
pub(crate) fn pick_smi_row(text: &str) -> Option<(String, u64, String)> {
    let mut best: Option<(String, u64, String)> = None;
    for line in text.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            continue;
        }
        let driver = parts[parts.len() - 1].to_string();
        let mb = parts[parts.len() - 2]
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);
        let name = parts[..parts.len() - 2].join(", ");
        if name.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(_, b, _)| mb > *b) {
            best = Some((name, mb, driver));
        }
    }
    best
}

/// Preference class for the WMI primary pick: integrated adapters (Intel
/// UHD/Iris/Arc and AMD APUs) lose to discrete cards no matter what WMI
/// enumerates first — a Ryzen laptop used to tier as its 780M iGPU even with
/// an RX 7600S installed. Pure + unit-tested.
pub(crate) fn gpu_primary_class(vendor: &str, name: &str) -> u8 {
    let g = name.to_uppercase();
    if vendor == "unknown" {
        // Only the driverless Basic Display Adapter gets here; it must never
        // beat a real adapter, but it must stay visible so the preflight can
        // refuse the install.
        return 0;
    }
    if vendor == "INTEL" {
        return 1;
    }
    const APU_TOKENS: [&str; 12] = [
        "680M", "660M", "610M", "780M", "760M", "740M", "890M", "880M", "860M", "820M", "8060S",
        "8050S",
    ];
    if APU_TOKENS.iter().any(|t| g.contains(t)) {
        return 1;
    }
    2
}

/// Primary non-NVIDIA controller: highest class (dGPU over iGPU), then
/// AdapterRAM (0/0xFFFFFFFF on big cards ties), then enumeration order.
#[cfg(windows)]
pub(crate) fn wmi_gpu_fallback() -> Option<(String, String, u64, String)> {
    let mut best: Option<(String, String, u64, String)> = None;
    for g in wmi_all_gpus() {
        let better = match &best {
            None => true,
            Some(b) => (gpu_primary_class(&g.1, &g.0), g.2) > (gpu_primary_class(&b.1, &b.0), b.2),
        };
        if better {
            best = Some(g);
        }
    }
    best
}
#[cfg(not(windows))]
pub(crate) fn wmi_gpu_fallback() -> Option<(String, String, u64, String)> {
    None
}

/// Win32_VideoController.AdapterRAM is a 32-bit field: cards with ≥4GB VRAM
/// report 0 or the 0xFFFFFFFF cap (≈4095MB) — a 32GB R9700 otherwise shows
/// up as a 4GB card (the exact user report). Either value is "VRAM unknown",
/// never truth. (A genuine sub-2GB iGPU is likewise unknown for tiering.)
/// Pure + cross-platform so unit tests cover it on any host.
pub(crate) fn adapter_ram_known_mb(raw: u64) -> Option<f64> {
    if raw == 0 || raw >= 0xFFF0_0000 {
        return None;
    }
    let mb = raw as f64 / (1024.0 * 1024.0);
    if !(2048.0..4095.0).contains(&mb) {
        None
    } else {
        Some(mb)
    }
}

/// AMD display-driver verdict from a Win32_VideoController DriverVersion
/// (`32.0.11029.1008` style). AMD moved to the 32.x store branch with the
/// 2024 / Adrenalin 24.x releases and kept it (Adrenalin marketing switched
/// to Year.Month, e.g. 26.9.1, while the store version stays 32.0.x — #15),
/// so major < 32 ≈ pre-24.x → update recommended (TheRock wants ≥ 24.5).
/// wants ≥ 24.5). Unparseable/empty → "unknown" (warn, don't block).
/// "Basic Display Adapter" is detected on the NAME by callers, not here.
/// Pure + unit-tested.
pub(crate) fn classify_amd_driver(version: &str) -> &'static str {
    let major: Option<u64> = version
        .trim()
        .split(['.', ' '])
        .next()
        .and_then(|m| m.parse().ok());
    match major {
        None => "unknown",
        Some(m) if m < 32 => "old",
        Some(_) => "ok",
    }
}

/// Distinctive GPU-name tokens: alphanumeric runs (len ≥ 4) containing a
/// digit — R9700, 9070, 7900XTX. Lets the registry match survive the small
/// WMI-vs-DriverDesc wording differences ("Radeon AI PRO R9700" vs
/// "AMD Radeon AI PRO R9700") without ever matching a generic iGPU entry.
fn distinctive_tokens(name: &str) -> Vec<String> {
    name.to_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 4 && t.bytes().any(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// Dedicated-VRAM fallback (MB) for known cards by name token.
/// The registry probe (wmi_dedicated_vram_mb) misses some driver layouts
/// (the R9700 reporter's PRO driver returned nothing) — for known cards
/// the size is fixed, so a name match beats "VRAM unknown" (which tiers a
/// 32GB card as 8GB downstream in setup.py's pid selection). Consulted
/// ONLY when WMI AdapterRAM and the registry both yield nothing, and only
/// on the non-NVIDIA path. Laptop S-suffixed SKUs may differ by a few GB —
/// tier-safe (thresholds are 11/22GB). Pure + unit-tested.
pub(crate) fn known_vram_mb(name: &str) -> Option<u64> {
    let g = name.to_uppercase();
    const GB: u64 = 1024;
    // Radeon PRO first: W7900/W7800/W6800 contain consumer tokens
    // ("7900"/"7800"/"6800") with DIFFERENT sizes.
    if g.contains("W7900") {
        return Some(48 * GB);
    }
    if g.contains("W7800") || g.contains("W6800") {
        return Some(32 * GB);
    }
    if g.contains("W7700") {
        return Some(16 * GB);
    }
    if g.contains("W6600") || g.contains("W6400") {
        return Some(8 * GB);
    }
    if g.contains("R9700") {
        return Some(32 * GB);
    }
    // RDNA 4 consumer (9070 GRE is the 12GB exception).
    // 9060 XT ships in 8GB and 16GB variants under the same name, so this
    // last-resort table under-tiers it: the registry probe normally reports
    // the true size, and when it fails an 8GB answer costs a slower P5 while
    // a 16GB answer OOMs on the 8GB card.
    if g.contains("9060") {
        return Some(8 * GB);
    }
    if g.contains("9070") {
        return Some(if g.contains("GRE") { 12 * GB } else { 16 * GB });
    }
    // RDNA 3 dGPU.
    if g.contains("7900") {
        if g.contains("XTX") {
            return Some(24 * GB);
        }
        if g.contains("GRE") {
            return Some(16 * GB);
        }
        return Some(20 * GB); // 7900 XT
    }
    if g.contains("7800") {
        return Some(16 * GB);
    }
    if g.contains("7700") {
        return Some(12 * GB);
    }
    if g.contains("7600") {
        return Some(if g.contains("XT") { 16 * GB } else { 8 * GB });
    }
    // RDNA 2 dGPU.
    if g.contains("6950") || g.contains("6900") || g.contains("6800") {
        return Some(16 * GB);
    }
    if g.contains("6750") || g.contains("6700") {
        return Some(12 * GB);
    }
    if g.contains("6650") || g.contains("6600") {
        return Some(8 * GB);
    }
    if g.contains("6500") || g.contains("6400") {
        return Some(4 * GB);
    }
    None
}

/// 64-bit dedicated VRAM (MB) from the display-driver registry key.
/// WMI AdapterRAM is uint32 (caps ~4GB, often 0); the driver also reports
/// the size as a QWORD — exact for 32GB cards. Consumer drivers expose it
/// as HardwareInformation.MemorySize, AMD PRO drivers (R9700 reporter box)
/// as HardwareInformation.qwMemorySize — read both, first parseable wins.
/// Fast (<300ms, detection paths only, never the metrics tick), no new deps.
/// Returns None when absent/implausible — callers keep "VRAM unknown".
#[cfg(windows)]
pub(crate) fn wmi_dedicated_vram_mb(display_name: &str) -> Option<u64> {
    let out = probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command",
        "Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}' | ForEach-Object { $d = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue; if ($d.DriverDesc) { $d.DriverDesc.ToString() + '|' + $d.'HardwareInformation.MemorySize' + '|' + $d.'HardwareInformation.qwMemorySize' } }"
    ]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let want = display_name.to_lowercase();
    let want_toks = distinctive_tokens(display_name);
    let mut cands: Vec<(String, u64)> = Vec::new();
    for ln in s.lines() {
        let mut parts = ln.split('|');
        let Some(desc) = parts.next() else {
            continue;
        };
        // First parseable QWORD wins (MemorySize on consumer, qwMemorySize
        // on PRO drivers; single-pipe legacy output keeps working).
        let mut mb: Option<u64> = None;
        for mem in parts {
            if let Ok(bytes) = mem.trim().parse::<u64>() {
                if let Some(m) = bytes.checked_div(1024 * 1024) {
                    if m != 0 && m <= 262144 {
                        mb = Some(m);
                        break;
                    }
                }
            }
        }
        let Some(mb) = mb else {
            continue;
        };
        let d = desc.trim().to_lowercase();
        if d.is_empty() || d.contains("nvidia") || is_virtual_display_adapter(&d) {
            continue;
        }
        cands.push((d, mb));
    }
    // 1) Name match (WMI vs DriverDesc can differ slightly); max wins
    // so a duplicated driver key can't under-report (mirrors upstream
    // setup.py get_adapter_vram_gb, which takes the max qwMemorySize).
    {
        let mut best: Option<u64> = None;
        for (d, mb) in &cands {
            if d.contains(&want) || want.contains(d) {
                best = Some(best.map_or(*mb, |b: u64| b.max(*mb)));
            }
        }
        if let Some(mb) = best {
            return Some(mb);
        }
    }
    // 2) Distinctive-token match (R9700/9070/…); max VRAM wins so a
    // duplicated driver key can't under-report, and a generic iGPU entry
    // (no digit tokens) can never match.
    if !want_toks.is_empty() {
        let mut best: Option<u64> = None;
        for (d, mb) in &cands {
            let dtoks = distinctive_tokens(d);
            if want_toks.iter().any(|t| dtoks.contains(t)) {
                best = Some(best.map_or(*mb, |b: u64| b.max(*mb)));
            }
        }
        if let Some(mb) = best {
            return Some(mb);
        }
    }
    // 3) Single-candidate fallback only — never attribute an iGPU's VRAM
    // to a dGPU or vice versa.
    if cands.len() == 1 {
        return Some(cands[0].1);
    }
    None
}
#[cfg(not(windows))]
pub(crate) fn wmi_dedicated_vram_mb(_display_name: &str) -> Option<u64> {
    None
}

// ── existing spike commands (kept) ──
#[tauri::command]
pub fn detect_gpu() -> serde_json::Value {
    get_gpu_info_sync()
}
pub(crate) fn kernel_profile_key(vendor: &str, name: &str) -> String {
    let v = vendor.to_uppercase();
    let g = name.to_uppercase();
    if v == "APPLE" {
        return "MPS".into();
    }
    if v == "NVIDIA" {
        // Pascal and older — last generation without attention kernels.
        if g.contains(" 10") || g.contains(" 16") || g.contains("GTX 10") || g.contains("GTX 16") {
            return "GTX_10".into();
        }
        // Workstation / datacenter BEFORE consumer tokens: "RTX 5000 Ada"
        // contains "RTX 50" but is Ada (RTX_40), not Blackwell — and
        // "RTX PRO 6000 Blackwell" matches no consumer token at all
        // (old code fell through to GTX_10: #2278, 96GB card mistiered).
        // Blackwell workstation + datacenter (cu130 torch ships sm_100/120).
        if g.contains("PRO 6000")
            || g.contains("PRO 5000")
            || g.contains("PRO 4000")
            || g.contains("B100")
            || g.contains("B200")
            || g.contains("GB100")
        {
            return "RTX_50".into();
        }
        // Hopper datacenter (cu130 torch ships sm_90 kernels).
        if g.contains("H100") || g.contains("H200") {
            return "RTX_50".into();
        }
        // Ada workstation (incl. L40/L4 datacenter) + consumer Ada caught
        // by the spaced token below; ADA rule first for WS names.
        if g.contains("ADA") || g.contains("L40") || g.contains(" L4") {
            return "RTX_40".into();
        }
        // Ampere workstation (Ax000 / Ax0 are Ampere, not Ada).
        if g.contains("RTX A")
            || g.contains(" A40")
            || g.contains(" A30")
            || g.contains(" A16")
            || g.contains(" A10")
            || g.contains(" A80")
        {
            return "RTX_30".into();
        }
        // Consumer GeForce — spaced generation tokens, so "RTX 3050"
        // (Ampere) can't match bare "50" and land on Blackwell (the old
        // check order did exactly that: 3050/4050 → RTX_50).
        if g.contains("RTX 50") || g.contains("RTX50") {
            return "RTX_50".into();
        }
        if g.contains("RTX 40") || g.contains("RTX40") {
            return "RTX_40".into();
        }
        if g.contains("RTX 30") || g.contains("RTX30") {
            return "RTX_30".into();
        }
        if g.contains("RTX 20") || g.contains("RTX20") || g.contains("QUADRO") {
            return "RTX_20".into();
        }
        if g.contains("20") {
            return "RTX_20".into();
        }
        return "GTX_10".into();
    }
    if v == "AMD" {
        // RDNA 2 (gfx103X): dGPUs plus the 680M/660M/610M APUs. No upstream
        // setup_config profile — dedicated key so install/launch treat it per
        // docs/AMD-INSTALLATION.md instead of GFX110X.
        if g.contains("GFX103")
            || g.contains("RX 6")
            || [
                "6300", "6400", "6450", "6500", "6600", "6650", "6700", "6750", "6800", "6850",
                "6900", "6950", "W6200", "W6400", "W6600", "W6800",
                // APUs (gfx1035/gfx1037) report "Radeon(TM) 680M" & co.
                "680M", "660M", "610M",
            ]
            .iter()
            .any(|x| g.contains(x))
        {
            return "AMD_GFX103X".into();
        }
        // RDNA 3 (gfx110X): dGPUs, the 780M/760M/740M APUs and the ROG Ally's
        // Z1 (gfx1103 — Z1 Extreme is a 780M-class part, NOT Strix Point).
        if ["7600", "7700", "7800", "7900", "780M", "760M", "740M", "Z1"]
            .iter()
            .any(|x| g.contains(x))
        {
            return "AMD_GFX110X".into();
        }
        // RDNA 3.5 APUs (gfx1150/1151/1152/1153): Strix Point (890M/880M),
        // Strix Halo (8060S/8050S), Ryzen AI 7 350 (860M), 840M/820M.
        // One launcher tier — upstream's unified `AMD` profile — and
        // amd_fallback_target narrows the device target by name.
        if [
            "890M", "880M", "860M", "840M", "820M", "8060S", "8050S", "STRIX", "HALO", "AI MAX",
        ]
        .iter()
        .any(|x| g.contains(x))
        {
            return "AMD_GFX1151".into();
        }
        // RDNA 4: Navi 44 = gfx1200, Navi 48 = gfx1201 (incl. PRO R9700).
        if g.contains("9060") || g.contains("1200") {
            return "AMD_GFX1200".into();
        }
        if g.contains("9070") || g.contains("9700") || g.contains("1201") {
            return "AMD_GFX1201".into();
        }
        return "AMD_GFX110X".into();
    }
    // Intel → INTEL_XPU key, but CPU torch (no XPU backend exists upstream;
    // overview shows INTEL_CPU via hardware_profile_detail). Unknown → CPU.
    // Never alias an NVIDIA profile: the overview would promise CUDA wheels
    // the installer never installs (this is where kernel-resolver.js kernelProfileKey went).
    if v == "INTEL" {
        return "INTEL_XPU".into();
    }
    "CPU".into()
}

/// Windows reports some APUs only as a generic adapter name, carrying no model
/// token the tier/device-target tables can match ("AMD Radeon(TM) Graphics" on
/// Ryzen 7000/8000 desktop parts and the ROG Ally). Callers must not guess a
/// gfx target for these. Pure + unit-tested.
pub(crate) fn amd_name_is_generic(name: &str) -> bool {
    let g = name.trim().to_uppercase();
    matches!(
        g.as_str(),
        "AMD RADEON(TM) GRAPHICS"
            | "AMD RADEON GRAPHICS"
            | "RADEON(TM) GRAPHICS"
            | "RADEON GRAPHICS"
    )
}

/// Upstream's attention mode per GPU class.
///
/// v17.01 states plainly that "quite a few optimizations depends on" Sage2/2+,
/// so this belongs alongside the memory profiles in a recommendation. It stays
/// a RECOMMENDATION: an `attention_mode` already on disk is shown as `saved`
/// and only changes if the user applies. sage3 is out (needs Blackwell and
/// py>=3.12; Sync GPU Wheels gates it off). None for AMD/Intel — upstream has
/// no Sage build on the ROCm path, which defaults to sdpa. Pure + unit-tested.
pub(crate) fn attention_for_profile(profile_key: &str) -> Option<&'static str> {
    match profile_key {
        // sm89 (Ada/Ampere) and sm120 (Blackwell) — Sage 2.2.0
        "RTX_50" | "RTX_40" | "RTX_30" => Some("sage2"),
        // Turing — SageAttention 1.0.6
        "RTX_20" => Some("sage"),
        // "Sage does not support GTX 10XX in WanGP"
        "GTX_10" => Some("sdpa"),
        _ => None,
    }
}

/// Docs-prescribed GGUF kernel floor: 1.0.28 (v18.20, 2026-10-21) is a
/// version-only move of the same three wheels. 1.0.27 reworked the
/// llama.cpp row loop into a single-launch shape — one CUDA-graph kernel node
/// and one memset node instead of a per-row launch, with per-shape first-use
/// tuning — for 2–10% faster decode (5–19% once a bias is added) and lower
/// VRAM; 1.0.28 adds the RTX 20 win: Bonsai and other PTQ1_0 language models
/// decode their tokens and speculative drafts faster, each block of weights
/// being decoded once for all the tokens of a step. It keeps 1.0.26's packed
/// rows crossing quantization blocks for
/// Q2_K through Q6_K, Q4_0/Q4_1, Q5_0/Q5_1 and Q8_0 (fixing MiniMax H3
/// image-reference projections with no dense-weight materialization) and
/// 1.0.25's INT8-verified speculative decoding for Q4_K / Bonsai PTQ1_0
/// (9% faster Qwen3.8 Q4 vs 1.0.23, 15% faster Bonsai vs 1.0.24 on RTX 5090)
/// plus the precompiled RTX50xx (SM120) async-copy kernels for Q8 prefill and
/// decode/verification (docs/INSTALLATION.md#gguf-llamacpp-cuda-kernels);
/// 1.0.25+ is required for Deepy Prime Bonsai PTQ1 at ~10GB VRAM. Older builds
/// silently fall back to slow PyTorch SDPA on Deepy decode (6 tok/s vs 37:
/// #2274, #2193), hit the Q2_K fallback assert (#2235), or 404 outright on
/// stale URLs (#2249).
/// Anything older resolves to the known-good 1.0.28 wheel; the floor,
/// newer builds, and unparseable URLs pass through, so the day upstream
/// flips setup_config forward we follow it with no code change.
/// HIP gets none of the CUDA-only single-launch win (upstream's
/// `linear_chunked_supported` is False on the ROCm backend) — the HIP pin is
/// a version-only bump.
pub(crate) const GGUF_FLOOR: &str = "1.0.29";

/// Numeric dotted-version compare: true when a > b ("1.0.21" > "1.0.2",
/// "3.8.0" > "3.3", "3.3.1" > "3.3"). Non-numeric tails ignored,
/// missing components count as zero. Pure + unit-tested.
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> Vec<u64> {
        s.split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    }
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (
            pa.get(i).copied().unwrap_or(0),
            pb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// Dist version out of a llamacpp_gguf_cuda wheel URL
/// (`.../llamacpp_gguf_cuda-1.0.14+torch210cu130py311-...whl` → "1.0.14").
/// Upstream percent-encodes the build tag (`%2B` for `+`) — cut at both, so
/// the floor math never compares "1.0.23%2Btorch…" as a version.
/// None for non-GGUF URLs. Pure + unit-tested.
pub(crate) fn gguf_wheel_version(url: &str) -> Option<String> {
    let file = url.rsplit('/').next()?;
    if !file.starts_with("llamacpp_gguf_cuda-") {
        return None;
    }
    let ver = file.split('-').nth(1)?.split(['+', '%']).next()?;
    if ver.is_empty() || !ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(ver.to_string())
}

/// Highest GGUF version pinned anywhere in setup_config's kernel components
/// (`gguf` + `gguf_cu128` win cmds). None when absent/unparseable — callers
/// keep the hardcoded floor. Pure + unit-tested.
pub(crate) fn setup_config_gguf_version(cfg: &serde_json::Value) -> Option<String> {
    let kernels = cfg.get("components")?.get("kernels")?.as_object()?;
    let mut best: Option<String> = None;
    for key in ["gguf", "gguf_cu128"] {
        let v = kernels
            .get(key)
            .and_then(|e| e.get("cmd"))
            .and_then(|c| c.get("win"))
            .and_then(|u| u.as_str())
            .and_then(gguf_wheel_version);
        if let Some(v) = v {
            if best.as_ref().is_none_or(|b| version_gt(&v, b)) {
                best = Some(v);
            }
        }
    }
    best
}

/// Effective GGUF floor: the hardcoded docs floor raised to whatever
/// setup_config already pins (max). Upstream now ships 1.0.28 in
/// setup_config — Sync/override/overview follow it with zero launcher
/// changes; stale checkouts still get the hardcoded floor. Pure +
/// unit-tested.
pub(crate) fn effective_gguf_floor(cfg: &serde_json::Value) -> String {
    let mut floor = GGUF_FLOOR.to_string();
    if let Some(v) = setup_config_gguf_version(cfg) {
        if version_gt(&v, &floor) {
            floor = v;
        }
    }
    floor
}

/// Warn-only structural validation of setup_config.json against the shapes
/// the launcher understands. Upstream adds components/profiles without
/// warning (gguf_cu128 in 5533384, light2xv earlier) — unknown entries must
/// shout in the console instead of silently skipping installs. Never blocks:
/// returns human-readable warnings, empty when clean. Pure + unit-tested.
pub(crate) fn validate_setup_config_shape(cfg: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let comps = cfg.get("components");
    if comps.is_none() {
        out.push("setup_config.json: missing 'components' — upstream shape changed; kernel sync may be incomplete (launcher may need an update)".to_string());
        return out;
    }
    let comps = comps.unwrap();
    let kernels = comps.get("kernels").and_then(|k| k.as_object());
    let profiles = cfg.get("gpu_profiles").and_then(|p| p.as_object());
    match profiles {
        None => out.push(
            "setup_config.json: missing 'gpu_profiles' — upstream shape changed; kernel sync may be incomplete (launcher may need an update)".to_string(),
        ),
        Some(profiles) => {
            for (pname, prof) in profiles {
                match prof.get("kernels").and_then(|k| k.as_array()) {
                    None => out.push(format!(
                        "setup_config.json: profile '{pname}' has no kernels list — skipping it; launcher may need an update"
                    )),
                    Some(list) => {
                        for k in list {
                            if let Some(key) = k.as_str() {
                                // Resolved outside components.kernels (sage via
                                // components.sage, sage3 gated off entirely).
                                if ["sage", "sageattention", "sage3", "sageattn3"]
                                    .contains(&key)
                                {
                                    continue;
                                }
                                let known = kernels
                                    .and_then(|m| m.get(key))
                                    .is_some();
                                if !known {
                                    out.push(format!(
                                        "setup_config.json: profile '{pname}' lists kernel '{key}' with no components.kernels entry — skipping it; launcher may need an update"
                                    ));
                                }
                            }
                        }
                    }
                }
                if let Some(sage_ver) = prof.get("sage").and_then(|s| s.as_str()) {
                    if comps.get("sage").and_then(|s| s.get(sage_ver)).is_none() {
                        out.push(format!(
                            "setup_config.json: profile '{pname}' wants sage '{sage_ver}' with no components.sage entry — sage sync skipped for this profile; launcher may need an update"
                        ));
                    }
                }
            }
        }
    }
    out
}
/// Pinned triton version out of a setup_config spec: `==3.3.1` pins and
/// `<3.3` ceilings both resolve to the bound ("3.3.1" / "3.3"); unpinned
/// specs and exact wheel URLs yield None (deliberate installs proceed).
/// Pure + unit-tested.
pub(crate) fn wanted_triton_pin(spec: &str) -> Option<String> {
    if spec.ends_with(".whl") {
        return None;
    }
    for (i, c) in spec.char_indices() {
        if c == '=' || c == '<' || c == '>' {
            let rest = spec[i..].trim_start_matches(['=', '<', '>', ' ', ',']);
            let ver: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let ver = ver.trim_matches('.').to_string();
            if ver.contains('.') {
                return Some(ver);
            }
            return None;
        }
    }
    None
}

/// Max Version: across `pip show` blocks (triton-windows + triton are
/// both reported — take the newest). None when unparseable.
/// Pure + unit-tested.
pub(crate) fn parse_pip_show_versions(text: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for ln in text.lines() {
        if let Some(v) = ln.trim().strip_prefix("Version:") {
            let v = v.trim().to_string();
            if !v.is_empty() && best.as_ref().is_none_or(|b| version_gt(&v, b)) {
                best = Some(v);
            }
        }
    }
    best
}

/// GGUF wheel URLs shipped by upstream (docs/INSTALLATION.md#gguf-llamacpp-cuda-kernels).
/// Upstream splits the component by (torch, python): `gguf` (cu130/py311),
/// `gguf_cu128` (cu128/py310) and `gguf_rocm10` (rocm10/py312, RDNA4-only).
/// Profiles still list `gguf`; setup.py remaps via (torch_k, py_k) — the
/// launcher mirrors that table in [`gguf_component_for`] (install.rs keys it
/// off the live env, status.rs off the profile), and the stale-pin floor below
/// still picks by the link's py tag.
/// Both CUDA consts carry upstream's `--no-deps` prefix verbatim (setup.py
/// installs `pip {cmd}` = `pip --no-deps <wheel>` — preserves the torch env);
/// install.rs splits the cmd into separate pip argv items.
const GGUF_1029_WIN_PY311: &str = "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
const GGUF_1029_WIN_PY310: &str = "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch271cu128py310-cp310-cp310-win_amd64.whl";
/// Upstream's GGUF matrix (setup.py:451-455): which `components.kernels`
/// entry a profile/env must install, keyed by its torch build and Python.
/// `None` = upstream publishes no wheel for that pair and skips the
/// component. `torch_kind` is either the profile's `torch` key (`rocm10`) or
/// the live env's build tag (`rocm100`, from `env_torch_kind`) — both accepted.
/// Pure + unit-tested.
pub(crate) fn gguf_component_for(torch_kind: &str, python: &str) -> Option<&'static str> {
    match (torch_kind, python) {
        ("cu130", "3.11") => Some("gguf"),
        ("cu128", "3.10") => Some("gguf_cu128"),
        ("rocm10" | "rocm100", "3.12") => Some("gguf_rocm10"),
        _ => None,
    }
}

/// True when a GGUF wheel URL is the AMD/HIP build — same dist name as the
/// CUDA wheel, different backend. The floor must NEVER swap it to a CUDA
/// wheel: the HIP wheel records and checks the exact ROCm torch build at
/// import, so a cu130/cu128 link would fail outright on an AMD box. Matches
/// every ROCm build tag (`rocm714` legacy → `rocm100` for 1.0.27+) plus `+hip`.
/// A bare `torch212` clause used to sit here too, but every AMD/HIP wheel
/// carries `rocm`/`+hip` and a future CUDA `torch212` build would have been
/// wrongly exempted from the floor.
fn is_amd_gguf_wheel(url_lower: &str) -> bool {
    url_lower.contains("rocm") || url_lower.contains("+hip")
}

/// Experimental AMD HIP wheel (upstream gguf-v1.0.29, llama.cpp/HIP.md):
/// `llamacpp_gguf_cuda-1.0.29+torch213rocm100py312` for RDNA4
/// (gfx1200/gfx1201). Same dist name as the CUDA wheel — pick by backend,
/// never mix. This build installs in the launcher's OWN AMD env (torch
/// 2.13.0+rocm10.0.0 / py3.12 — the stack Sync installs), so the separate
/// ROCm 7.14 environment is gone; Sync lands it via the `gguf_rocm10`
/// component and install_hip_gguf_wheel stays as the explicit opt-in /
/// reinstall path. The 1.0.25/1.0.26 HIP wheels were py311/torch2.10/rocm7.14
/// and are no longer what upstream ships.
pub(crate) const GGUF_1029_WIN_PY312_HIP: &str = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch213rocm100py312-cp312-cp312-win_amd64.whl";
/// GGUF floor toward the documented 1.0.28 build (the 1.0.27 single-launch
/// row loop — 2–10% faster decode, 5–19% with a bias, less VRAM — plus the
/// 1.0.28 RTX 20 PTQ1_0 / Bonsai decode win; keeps 1.0.26's packed
/// rows crossing quantization blocks for Q2_K..Q6_K / Q4_0…Q8_0, fixing
/// MiniMax H3 image-reference projections, and 1.0.25's INT8-verified
/// speculative decoding for Q4_K / Bonsai PTQ1_0 + RTX50 SM120 kernels).
/// setup_config.json lags in old checkouts, so anything below the floor
/// swaps to 1.0.28 — but the floor, anything newer, and non-GGUF URLs pass
/// through untouched, so the day upstream flips setup_config we follow it
/// verbatim with no code change (same shape as the Sage post4/post6 swap in
/// sync_kernels).
/// CUDA only: AMD/HIP pins are exempt (see [`is_amd_gguf_wheel`]) because the
/// ROCm build cannot be replaced by a CUDA link.
/// Applies to every kernel URL (no-op unless it's a stale GGUF link), so
/// both the sync installer and the overview's want/have comparison share it.
/// Plain hardcoded-floor wrapper. Prod paths with config use
/// `apply_gguf_override_cfg`; this stays as the stable contract for the
/// no-config paths (kernel probe stale check) and the JS mirror.
#[allow(dead_code)]
pub(crate) fn apply_gguf_override(url: &str) -> String {
    apply_gguf_override_with(url, GGUF_FLOOR)
}

/// Config-aware GGUF override (preferred wherever setup_config is loaded):
/// swaps stale pins to upstream's OWN fresh pin when setup_config already
/// carries one (py tag picks gguf vs gguf_cu128, mirroring setup.py), else
/// falls back to the hardcoded docs build. The AMD/HIP wheel owns itself and
/// is never touched here (see [`is_amd_gguf_wheel`]).
/// Pure + unit-tested.
pub(crate) fn apply_gguf_override_cfg(url: &str, cfg: &serde_json::Value) -> String {
    let low = url.to_ascii_lowercase();
    if is_amd_gguf_wheel(&low) {
        return url.to_string();
    }
    let Some(ver) = gguf_wheel_version(url) else {
        return url.to_string();
    };
    if !version_gt(&effective_gguf_floor(cfg), &ver) {
        return url.to_string();
    }
    gguf_swap_target(cfg, url)
}

/// Swap target for a stale CUDA GGUF pin: upstream's fresh component URL when
/// it meets the hardcoded floor, else the hardcoded docs build for the URL's
/// py tag. AMD/HIP links never get here — both callers bail out first via
/// [`is_amd_gguf_wheel`]. Pure + unit-tested.
pub(crate) fn gguf_swap_target(cfg: &serde_json::Value, url: &str) -> String {
    let comp = if url.contains("py310") {
        "gguf_cu128"
    } else {
        "gguf"
    };
    if let Some(up) = cfg
        .get("components")
        .and_then(|c| c.get("kernels"))
        .and_then(|m| m.get(comp))
        .and_then(|e| e.get("cmd"))
        .and_then(|c| c.get("win"))
        .and_then(|u| u.as_str())
    {
        if let Some(v) = gguf_wheel_version(up) {
            if !version_gt(GGUF_FLOOR, &v) {
                return up.to_string();
            }
        }
    }
    if url.contains("py310") {
        GGUF_1029_WIN_PY310.into()
    } else {
        GGUF_1029_WIN_PY311.into()
    }
}
/// Floor-parameterized GGUF override (pure floor compare + hardcoded docs
/// target). Prefer `apply_gguf_override_cfg` wherever setup_config is loaded
/// — only this form is used by no-config paths and unit tests.
pub(crate) fn apply_gguf_override_with(url: &str, floor: &str) -> String {
    // HIP stack owns its wheel (same dist name, different backend): never
    // swap HIP->CUDA or CUDA->HIP here. Sync installs the ROCm build via the
    // `gguf_rocm10` component; install_hip_gguf_wheel installs it directly.
    let low = url.to_ascii_lowercase();
    if is_amd_gguf_wheel(&low) {
        return url.to_string();
    }
    let Some(ver) = gguf_wheel_version(url) else {
        return url.to_string();
    };
    if !version_gt(floor, &ver) {
        return url.to_string();
    }
    if url.contains("py310") {
        GGUF_1029_WIN_PY310.into()
    } else {
        GGUF_1029_WIN_PY311.into()
    }
}

/// WGPKernels: the INT8 (and general) fast-kernel family behind upstream's
/// `int8_kernels = auto` ranking (wgpkernels > kitchen > triton > pytorch).
/// Since upstream 17307e5 (v1.1.2) the wheels are listed in WanGP's
/// `setup_config.json` (`components.kernels.wgpkernels*`), so Sync installs
/// them — the dashboard button stays as the explicit opt-in / reinstall path.
/// Upstream v18.20 (2026-10-21) moves to v1.3.0 and puts `wgpkernels` in the
/// RTX_20 profile too ("RTX 20XX Gets a Second Wind"): Turing is supported
/// from 1.3.0 on, and upstream checks the kernels on the actual GPU before
/// using them.
///
/// v1.1.1 assets were deleted upstream (404). `rocm100` (ROCm 10, RDNA4)
/// replaces the retired Windows-only `rocm714` and ships win+linux.
/// The wheel is `py3-none` (no CPython ABI tag), so one asset serves every
/// Python — only the CUDA/ROCm tag has to match the env's torch build.
/// NVIDIA RTX 20/30/40/50 qualify (Turing 7.5 and up); the
/// `rocm100` build is the RDNA4 (gfx1200/gfx1201) experimental path and
/// loads in the installer's torch 2.13+rocm10 env (no separate env).
pub(crate) const WGPKERNELS_VERSION: &str = "1.4.2";
pub(crate) const WGPKERNELS_BASE: &str =
    "https://github.com/deepbeepmeep/WGPKernels/releases/download/v1.4.2";

/// Resolve the WGPKernels wheel URL for a torch build tag and OS. `torch_kind`
/// is `cu130` / `cu128` (NVIDIA) or `rocm100` (AMD RDNA4, win+linux);
/// `os` is `win_amd64` / `linux_x86_64`. Anything else has no published wheel
/// (MPS/CPU). Pure + unit-tested.
pub(crate) fn wgpkernels_wheel_url(torch_kind: &str, os: &str) -> Option<String> {
    let ok_os = matches!(os, "win_amd64" | "linux_x86_64");
    let tag = match torch_kind {
        "cu130" | "cu128" => torch_kind,
        "rocm100" => "rocm100",
        _ => return None,
    };
    if !ok_os {
        return None;
    }
    Some(format!(
        "{WGPKERNELS_BASE}/wgpkernels-{WGPKERNELS_VERSION}%2B{tag}-py3-none-{os}.whl"
    ))
}

/// GPU profile keys that can run WGPKernels. Upstream v1.3.0 supports NVIDIA
/// RTX 20XX and newer — RTX 30/40/50 (Ampere and later, compute capability
/// 8.0 or higher) plus RTX_20 (Turing, 7.5, which upstream's own installer and
/// `gpu_profiles` now include) — and checks the kernels on the GPU before
/// using them. GTX_10 (Pascal, 6.1) stays below the floor. AMD RDNA4
/// (AMD_GFX1200 RX 9060/9060 XT + AMD_GFX1201 RX 9070/9070 XT/R9700) via the
/// experimental `rocm100` wheel. Pure + unit-tested.
pub(crate) fn wgpkernels_supported_profile(profile_key: &str) -> bool {
    matches!(
        profile_key,
        "RTX_20" | "RTX_30" | "RTX_40" | "RTX_50" | "AMD_GFX1200" | "AMD_GFX1201"
    )
}

#[cfg(test)]
mod wgpkernels_tests {
    use super::{wgpkernels_supported_profile, wgpkernels_wheel_url, WGPKERNELS_VERSION};
    #[test]
    fn cuda_and_rocm_urls_match_upstream() {
        assert_eq!(
            wgpkernels_wheel_url("cu130", "win_amd64").unwrap(),
            format!("https://github.com/deepbeepmeep/WGPKernels/releases/download/v{WGPKERNELS_VERSION}/wgpkernels-{WGPKERNELS_VERSION}%2Bcu130-py3-none-win_amd64.whl")
        );
        assert_eq!(
            wgpkernels_wheel_url("cu128", "linux_x86_64").unwrap(),
            format!("https://github.com/deepbeepmeep/WGPKernels/releases/download/v{WGPKERNELS_VERSION}/wgpkernels-{WGPKERNELS_VERSION}%2Bcu128-py3-none-linux_x86_64.whl")
        );
        // rocm100 ships win+linux (unlike the retired rocm714).
        assert_eq!(
            wgpkernels_wheel_url("rocm100", "win_amd64").unwrap(),
            format!("https://github.com/deepbeepmeep/WGPKernels/releases/download/v{WGPKERNELS_VERSION}/wgpkernels-{WGPKERNELS_VERSION}%2Brocm100-py3-none-win_amd64.whl")
        );
        assert!(wgpkernels_wheel_url("rocm100", "linux_x86_64").is_some());
        // Retired tags (v1.1.1 assets deleted upstream) resolve to nothing.
        for bad in ["rocm714", "rocm10"] {
            assert!(wgpkernels_wheel_url(bad, "win_amd64").is_none(), "{bad}");
        }
        // No wheel for CPU/MPS or unknown tags.
        for bad in ["cpu", "", "cu121", "mps"] {
            assert!(wgpkernels_wheel_url(bad, "win_amd64").is_none(), "{bad}");
        }
        assert!(wgpkernels_wheel_url("cu130", "macosx").is_none());
    }
    #[test]
    fn turing_and_newer_and_rdna4_profiles_qualify() {
        for good in [
            "RTX_20",
            "RTX_30",
            "RTX_40",
            "RTX_50",
            "AMD_GFX1200",
            "AMD_GFX1201",
        ] {
            assert!(wgpkernels_supported_profile(good), "{good}");
        }
        for bad in ["GTX_10", "AMD_GFX110X", "AMD_GFX103X", "CPU"] {
            assert!(!wgpkernels_supported_profile(bad), "{bad}");
        }
    }
}

#[cfg(test)]
mod amd_profile_tests {
    use super::kernel_profile_key;
    #[test]
    fn r9700_maps_to_gfx1201() {
        // Radeon AI PRO R9700 = gfx1201 (Navi 48, RDNA 4) — doc-leading per
        // docs/AMD-INSTALLATION.md; must not fall through to AMD_GFX110X.
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon AI PRO R9700"),
            "AMD_GFX1201"
        );
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 9070 XT"),
            "AMD_GFX1201"
        );
        // RX 9060 XT = gfx1200 (Navi 44), NOT gfx1201 — upstream table.
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 9060 XT"),
            "AMD_GFX1200"
        );
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 9060"),
            "AMD_GFX1200"
        );
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 7900 XTX"),
            "AMD_GFX110X"
        );
        // RDNA 2 has its own key (no upstream setup_config profile to collide with).
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 6800 XT"),
            "AMD_GFX103X"
        );
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon RX 6700S"),
            "AMD_GFX103X"
        );
    }
    #[test]
    fn apu_names_tier_by_architecture() {
        use super::{amd_name_is_generic, kernel_profile_key};
        // RDNA 3 APUs + ROG Ally (gfx1103) must not read as Strix Point.
        for n in [
            "AMD Radeon 780M",
            "AMD Radeon 760M",
            "AMD Radeon 740M",
            "AMD Radeon Graphics (Z1 Extreme)",
        ] {
            assert_eq!(kernel_profile_key("AMD", n), "AMD_GFX110X", "{n}");
        }
        // RDNA 3.5 APUs.
        for n in [
            "AMD Radeon 890M",
            "AMD Radeon 880M",
            "AMD Radeon 860M",
            "AMD Radeon 820M",
            "AMD Radeon(TM) 8060S Graphics",
            "AMD RADEON 8050S",
        ] {
            assert_eq!(kernel_profile_key("AMD", n), "AMD_GFX1151", "{n}");
        }
        // RDNA 2 APUs.
        for n in ["AMD Radeon 680M", "AMD Radeon 660M", "AMD Radeon 610M"] {
            assert_eq!(kernel_profile_key("AMD", n), "AMD_GFX103X", "{n}");
        }
        // Generic adapter name: only the preflight's target field can resolve
        // it, so the key stays the family default and the detector fires.
        assert!(amd_name_is_generic("AMD Radeon(TM) Graphics"));
        assert!(amd_name_is_generic("  AMD Radeon Graphics "));
        assert_eq!(
            kernel_profile_key("AMD", "AMD Radeon(TM) Graphics"),
            "AMD_GFX110X"
        );
        assert!(!amd_name_is_generic("AMD Radeon RX 7900 XTX"));
        assert!(!amd_name_is_generic("AMD Radeon 780M"));
    }
}
#[cfg(test)]
mod nvidia_profile_tests {
    use super::kernel_profile_key;
    #[test]
    fn consumer_generations() {
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 5090"),
            "RTX_50"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 5060 Ti"),
            "RTX_50"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 4070 Laptop GPU"),
            "RTX_40"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 4050 Laptop GPU"),
            "RTX_40"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 3080"),
            "RTX_30"
        );
        // Bare-"50"-first ordering trap: 3050 is Ampere, not Blackwell.
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 3050"),
            "RTX_30"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce RTX 2060"),
            "RTX_20"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce GTX 1080"),
            "GTX_10"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA GeForce GTX 1660 SUPER"),
            "GTX_10"
        );
    }
    #[test]
    fn workstation_and_datacenter() {
        // #2278: 96GB Blackwell workstation card fell through to GTX_10.
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA RTX PRO 6000 Blackwell"),
            "RTX_50"
        );
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA RTX 6000 Ada Generation"),
            "RTX_40"
        );
        // "RTX 5000 Ada" contains "RTX 50" — WS rule must win.
        assert_eq!(
            kernel_profile_key("NVIDIA", "NVIDIA RTX 5000 Ada Generation"),
            "RTX_40"
        );
        assert_eq!(kernel_profile_key("NVIDIA", "NVIDIA RTX A6000"), "RTX_30");
        assert_eq!(kernel_profile_key("NVIDIA", "NVIDIA RTX A4000"), "RTX_30");
        assert_eq!(kernel_profile_key("NVIDIA", "NVIDIA H100"), "RTX_50");
        assert_eq!(kernel_profile_key("NVIDIA", "NVIDIA A100"), "RTX_30");
        assert_eq!(kernel_profile_key("NVIDIA", "Quadro RTX 6000"), "RTX_20");
    }
}
#[cfg(test)]
mod gpu_pick_tests {
    use super::{gpu_primary_class, parse_nv_cards, pick_smi_row, resolve_gpu_pick, NvCard};
    fn cards() -> Vec<NvCard> {
        vec![
            NvCard {
                index: 0,
                uuid: "GPU-aaa".into(),
                name: "NVIDIA GeForce RTX 4070".into(),
                vram_mb: 8188,
                free_mb: 7000,
                driver: "566.03".into(),
            },
            NvCard {
                index: 1,
                uuid: "GPU-bbb".into(),
                name: "NVIDIA GeForce RTX 4090".into(),
                vram_mb: 24564,
                free_mb: 900,
                driver: "566.03".into(),
            },
        ]
    }
    #[test]
    fn smi_card_rows_parse_and_sort() {
        // Two cards, a comma inside one name, memory/free/driver right-aligned.
        let text = "1, GPU-bbb, NVIDIA GeForce RTX 4090, 24564, 900, 566.03\n\
                    0, GPU-aaa, NVIDIA RTX A5000, Ada, 24564, 100, 537.99";
        let parsed = parse_nv_cards(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].index, 0, "rows sort by nvidia-smi index");
        assert_eq!(parsed[0].uuid, "GPU-aaa");
        assert_eq!(parsed[0].name, "NVIDIA RTX A5000, Ada");
        assert_eq!(parsed[0].vram_mb, 24564);
        assert_eq!(parsed[0].free_mb, 100);
        assert_eq!(parsed[0].driver, "537.99");
        assert_eq!(parsed[1].index, 1);
        // Legacy 3-field output is not a card row (pick_smi_row owns that).
        assert!(parse_nv_cards("NVIDIA GeForce RTX 3080, 10240 MiB, 546.01").is_empty());
        assert!(parse_nv_cards("").is_empty());
    }
    #[test]
    fn auto_pins_the_biggest_card_by_index() {
        let pin = resolve_gpu_pick("auto", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
        assert_eq!(pin.card.as_ref().unwrap().vram_mb, 24564);
        assert!(pin.note.is_none());
    }
    #[test]
    fn legacy_smi_index_maps_to_the_same_physical_card() {
        // The old picker stored "cuda:<nvidia-smi index>"; CUDA's default
        // FASTEST_FIRST order can point that ordinal at another card, so the
        // launcher resolves the index to the card and passes its index back.
        let pin = resolve_gpu_pick("cuda:0", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:0"));
        assert_eq!(pin.card.as_ref().unwrap().name, "NVIDIA GeForce RTX 4070");
        let pin = resolve_gpu_pick("uuid:GPU-bbb", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
    }
    #[test]
    fn all_and_stale_picks_are_explicit() {
        // "all" leaves every GPU visible (advanced multi-GPU).
        let pin = resolve_gpu_pick("all", &cards());
        assert_eq!(pin.arg, None);
        // A removed card falls back to the best card instead of hiding every
        // GPU (torch would see zero devices), and the note names the ghost.
        let pin = resolve_gpu_pick("cuda:9", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
        assert!(pin.note.unwrap().contains("cuda:9"));
        let pin = resolve_gpu_pick("uuid:GPU-gone", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
        assert!(pin.note.unwrap().contains("GPU-gone"));
        // No nvidia-smi: there is no ordinal to pass, so WanGP keeps its own
        // default device instead of being handed a guess.
        let pin = resolve_gpu_pick("uuid:GPU-gone", &[]);
        assert_eq!(pin.arg, None);
        assert!(pin.card.is_none());
        // Garbage never silently picks device 0 of an unknown list.
        let pin = resolve_gpu_pick("banana", &cards());
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
        assert!(pin.note.unwrap().contains("banana"));
        // No NVIDIA cards (AMD/CPU box): no CUDA arg at all.
        let pin = resolve_gpu_pick("auto", &[]);
        assert_eq!(pin.arg, None);
        assert!(pin.card.is_none());
    }
    #[test]
    fn smi_rows_pick_the_largest_card() {
        // Two cards: the old blob-split put the second name into
        // driverVersion. Names may contain commas, memory is right-aligned.
        let multi = "NVIDIA GeForce RTX 4070, 8188 MiB, 566.03\nNVIDIA GeForce RTX 4090, 24564 MiB, 566.03\n";
        assert_eq!(
            pick_smi_row(multi),
            Some(("NVIDIA GeForce RTX 4090".into(), 24564, "566.03".into()))
        );
        let single = "NVIDIA GeForce RTX 3080, 10240 MiB, 546.01";
        assert_eq!(
            pick_smi_row(single),
            Some(("NVIDIA GeForce RTX 3080".into(), 10240, "546.01".into()))
        );
        let comma = "NVIDIA RTX A5000, 24564 MiB, 537.99";
        assert_eq!(
            pick_smi_row(comma),
            Some(("NVIDIA RTX A5000".into(), 24564, "537.99".into()))
        );
        assert_eq!(pick_smi_row(""), None);
        assert_eq!(pick_smi_row("no csv here"), None);
    }
    #[test]
    fn integrated_adapters_lose_to_discrete() {
        // Ryzen laptop order: the 780M iGPU is enumerated first, the dGPU
        // second — the dGPU must win or tiering runs on the iGPU.
        assert_eq!(gpu_primary_class("AMD", "AMD Radeon 780M"), 1);
        assert_eq!(gpu_primary_class("AMD", "AMD Radeon RX 7600S"), 2);
        assert_eq!(gpu_primary_class("INTEL", "Intel Arc A770 Graphics"), 1);
        assert_eq!(gpu_primary_class("AMD", "AMD Radeon 880M"), 1);
        assert_eq!(gpu_primary_class("AMD", "AMD Radeon RX 9070 XT"), 2);
        assert_eq!(gpu_primary_class("AMD", "AMD Radeon AI PRO R9700"), 2);
        // Driverless boxes: the Basic Display Adapter is surfaced (so the
        // preflight can refuse) but ranks below every real adapter.
        assert_eq!(
            gpu_primary_class("unknown", "Microsoft Basic Display Adapter"),
            0
        );
    }
}

#[cfg(test)]
mod virtual_adapter_tests {
    use super::{is_amd_apu, is_virtual_display_adapter};
    #[test]
    fn amd_apu_vs_discrete() {
        // APUs share system memory and are named "Radeon Graphics".
        for n in [
            "AMD Radeon Graphics",
            "AMD Radeon 780M Graphics",
            "AMD Radeon(TM) Graphics",
            "AMD Radeon 680M",
        ] {
            assert!(is_amd_apu(n), "{n} should be an APU");
        }
        // Discrete cards must never be classified as APUs.
        for n in [
            "AMD Radeon RX 7900 XTX",
            "AMD Radeon RX 9070 XT",
            "AMD Radeon PRO W7900",
            "AMD Radeon VII",
            "AMD Radeon RX Vega 64",
            "AMD Radeon Instinct MI300X",
            "NVIDIA GeForce RTX 3080",
            "Intel UHD Graphics 770",
        ] {
            assert!(!is_amd_apu(n), "{n} should not be an APU");
        }
    }
    #[test]
    fn virtual_names_detected() {
        // #2224 reporter names first.
        for n in [
            "GameViewer Virtual Display Adapter",
            "ToDesk Virtual Display Adapter",
            "Parsec Virtual Display Adapter",
            "Microsoft Remote Display Adapter",
            "Microsoft Hyper-V Video",
            "VMware SVGA 3D",
            "VirtualBox Graphics Adapter",
            "Indirect Display Adapter",
            "Citrix Indirect Display Adapter",
        ] {
            assert!(is_virtual_display_adapter(n), "{n}");
        }
    }
    #[test]
    fn physical_names_not_virtual() {
        for n in [
            "AMD Radeon RX 7900 XTX",
            "NVIDIA GeForce RTX 3080",
            "NVIDIA RTX PRO 6000 Blackwell",
            "Intel UHD Graphics 770",
            "Intel Arc A770 Graphics",
            // No-driver state has its own fatal check — not virtual.
            "Microsoft Basic Display Adapter",
            "",
        ] {
            assert!(!is_virtual_display_adapter(n), "{n}");
        }
    }
}
#[cfg(test)]
mod amd_driver_tests {
    use super::classify_amd_driver;
    #[test]
    fn driver_verdicts() {
        // 32.x store branch (Adrenalin 24.x–26.x+) — current.
        assert_eq!(classify_amd_driver("32.0.11029.1008"), "ok");
        assert_eq!(classify_amd_driver("32.0.12019.1028"), "ok");
        // #15 reporter's Adrenalin 26.9.1 (store 32.0.31041.3013) — must not mislabel.
        assert_eq!(classify_amd_driver("32.0.31041.3013"), "ok");
        // 23.x era (31.x) — predates the 24.5 TheRock floor: warn.
        assert_eq!(classify_amd_driver("31.0.21029.1006"), "old");
        assert_eq!(classify_amd_driver("30.0.13025.1000"), "old");
        // Unreadable — warn, never block.
        assert_eq!(classify_amd_driver(""), "unknown");
        assert_eq!(classify_amd_driver("not-a-version"), "unknown");
    }
}
#[cfg(test)]
mod known_vram_tests {
    use super::known_vram_mb;
    #[test]
    fn known_cards_resolve() {
        // The 0.5.1 reporter card: registry probe missed, table must hit.
        assert_eq!(known_vram_mb("AMD Radeon AI PRO R9700"), Some(32768));
        assert_eq!(known_vram_mb("AMD Radeon RX 9070 XT"), Some(16384));
        // Ambiguous 8/16GB SKU: the fallback under-tiers (safe direction).
        assert_eq!(known_vram_mb("AMD Radeon RX 9060 XT"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon RX 9060"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon RX 9070 GRE"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 7900 XTX"), Some(24576));
        assert_eq!(known_vram_mb("AMD Radeon RX 7900 XT"), Some(20480));
        assert_eq!(known_vram_mb("AMD Radeon RX 7800 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 7700 XT"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 7600"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon RX 7600 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 6800 XT"), Some(16384));
        assert_eq!(known_vram_mb("AMD Radeon RX 6700 XT"), Some(12288));
        assert_eq!(known_vram_mb("AMD Radeon RX 6600"), Some(8192));
        assert_eq!(known_vram_mb("AMD Radeon PRO W7900"), Some(49152));
        assert_eq!(known_vram_mb("AMD Radeon PRO W7800"), Some(32768));
    }
    #[test]
    fn unknown_names_stay_unknown() {
        assert_eq!(known_vram_mb("AMD Radeon Graphics"), None); // generic iGPU
        assert_eq!(known_vram_mb("Intel Arc A770"), None);
        assert_eq!(known_vram_mb(""), None);
    }
}
#[cfg(test)]
mod gguf_override_tests {
    use super::apply_gguf_override;
    #[test]
    fn swaps_stale_for_1028() {
        let old_win = "https://github.com/deepbeepmeep/kernels/releases/download/GGUF_Kernels/llamacpp_gguf_cuda-1.0.14+torch210cu130py311-cp311-cp311-win_amd64.whl";
        let new = apply_gguf_override(old_win);
        assert!(
            new.contains("gguf-v1.0.29") && new.contains("1.0.29"),
            "got {new}"
        );
        assert!(!new.contains("1.0.14"));
        let old_310 = old_win
            .replace("py311", "py310")
            .replace("torch210cu130py311", "torch271cu128py310");
        assert!(apply_gguf_override(&old_310).contains("torch271cu128py310"));
        // 1.0.21 through 1.0.28 are now below the floor (docs prescribe 1.0.29).
        for v in [
            "1.0.21", "1.0.22", "1.0.23", "1.0.24", "1.0.25", "1.0.26", "1.0.27", "1.0.28",
        ] {
            let old = old_win.replace("1.0.14", v);
            let swapped = apply_gguf_override(&old);
            assert!(
                swapped.contains("gguf-v1.0.29") && swapped.contains("1.0.29"),
                "got {swapped}"
            );
        }
    }
    #[test]
    fn passes_other_urls_through() {
        // At the floor and newer both follow upstream with no swap.
        for u in [
            "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29+torch210cu130py311-cp311-cp311-win_amd64.whl",
            "https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl",
        ] { assert_eq!(apply_gguf_override(u), u); }
    }
    #[test]
    fn floor_catches_all_stale_builds() {
        // #2274 (1.0.2 silent SDPA fallback), #2235, #2249 (1.0.13 404):
        // anything below 1.0.29 resolves to the known-good wheel.
        for v in [
            "1.0.2", "1.0.7", "1.0.12", "1.0.13", "1.0.14", "1.0.21", "1.0.22", "1.0.23", "1.0.24",
            "1.0.25", "1.0.26", "1.0.27", "1.0.28",
        ] {
            let u = format!("https://github.com/deepbeepmeep/kernels/releases/download/GGUF_Kernels/llamacpp_gguf_cuda-{v}+torch210cu130py311-cp311-cp311-win_amd64.whl");
            let out = apply_gguf_override(&u);
            assert!(
                out.contains("gguf-v1.0.29") && out.contains("1.0.29") && out.contains("cp311"),
                "{v} got {out}"
            );
        }
        // py310 tag keeps the py310 variant.
        let old310 = "https://github.com/deepbeepmeep/kernels/releases/download/GGUF_Kernels/llamacpp_gguf_cuda-1.0.12+torch271cu128py310-cp310-cp310-win_amd64.whl";
        let out310 = apply_gguf_override(old310);
        assert!(
            out310.contains("torch271cu128py310") && out310.contains("1.0.29"),
            "got {out310}"
        );
    }
    #[test]
    fn amd_hip_wheels_are_never_swapped() {
        // The ROCm build shares the CUDA wheel's dist name but not its
        // backend: the floor must leave it alone in BOTH forms, or a sync
        // would write a cu130 link into an env whose wheel checks the ROCm
        // torch build at import.
        use super::{apply_gguf_override_cfg, apply_gguf_override_with};
        let rocm100 = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch213rocm100py312-cp312-cp312-win_amd64.whl";
        // Stale-looking: 1.0.7 is far below the 1.0.28 floor.
        let stale_hip = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.7/llamacpp_gguf_cuda-1.0.7%2Btorch210rocm714py311-cp311-cp311-win_amd64.whl";
        let cfg = serde_json::json!({ "components": { "kernels": {
            "gguf": { "cmd": { "win": "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch210cu130py311-cp311-cp311-win_amd64.whl" } }
        } } });
        for u in [rocm100, stale_hip] {
            assert_eq!(apply_gguf_override(u), u);
            assert_eq!(apply_gguf_override_with(u, "9.9.9"), u);
            assert_eq!(apply_gguf_override_cfg(u, &cfg), u);
        }
    }
    #[test]
    fn gguf_component_matrix_mirrors_setup_py() {
        use super::gguf_component_for;
        assert_eq!(gguf_component_for("cu130", "3.11"), Some("gguf"));
        assert_eq!(gguf_component_for("cu128", "3.10"), Some("gguf_cu128"));
        // Profile key (`rocm10`) and live env build tag (`rocm100`) both map.
        assert_eq!(gguf_component_for("rocm10", "3.12"), Some("gguf_rocm10"));
        assert_eq!(gguf_component_for("rocm100", "3.12"), Some("gguf_rocm10"));
        // Unpublished pairs: upstream skips the component instead.
        for (t, p) in [
            ("cu130", "3.10"),
            ("cu128", "3.11"),
            ("rocm100", "3.11"),
            ("", ""),
        ] {
            assert_eq!(gguf_component_for(t, p), None, "{t}/{p}");
        }
    }
    #[test]
    fn effective_floor_follows_setup_config_forward() {
        use super::{apply_gguf_override_cfg, effective_gguf_floor, setup_config_gguf_version};
        // Current upstream shape: both components at 1.0.28 → floor stays.
        let cfg = serde_json::json!({
            "components": { "kernels": {
                "gguf": { "cmd": { "win": "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch210cu130py311-cp311-cp311-win_amd64.whl" } },
                "gguf_cu128": { "cmd": { "win": "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch271cu128py310-cp310-cp310-win_amd64.whl" } }
            } }
        });
        assert_eq!(setup_config_gguf_version(&cfg).as_deref(), Some("1.0.29"));
        assert_eq!(effective_gguf_floor(&cfg), "1.0.29");
        // Upstream ships 1.0.29 tomorrow: floor follows with no code change,
        // and 1.0.28 installs now flag stale.
        let cfg29 = serde_json::json!({
            "components": { "kernels": {
                "gguf": { "cmd": { "win": "--no-deps https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch210cu130py311-cp311-cp311-win_amd64.whl" } }
            } }
        });
        assert_eq!(effective_gguf_floor(&cfg29), "1.0.29");
        let u28 = "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.29/llamacpp_gguf_cuda-1.0.29%2Btorch210cu130py311-cp311-cp311-win_amd64.whl";
        let swapped = apply_gguf_override_cfg(u28, &cfg29);
        assert!(swapped.contains("1.0.29"), "got {swapped}");
        // Stale checkout (1.0.14 pins, no cu128): hardcoded floor still applies.
        let stale = serde_json::json!({
            "components": { "kernels": {
                "gguf": { "cmd": { "win": "https://github.com/deepbeepmeep/kernels/releases/download/GGUF_Kernels/llamacpp_gguf_cuda-1.0.14+torch210cu130py311-cp311-cp311-win_amd64.whl" } }
            } }
        });
        assert_eq!(effective_gguf_floor(&stale), super::GGUF_FLOOR);
    }
    #[test]
    fn shape_validation_flags_unknown_kernels() {
        use super::validate_setup_config_shape;
        let cfg = serde_json::json!({
            "components": {
                "kernels": { "gguf": { "cmd": { "win": "https://x/gguf-1.0.23.whl" } } },
                "sage": { "v220_cu13": { "cmd": { "win": "https://x/sage.whl" } } }
            },
            "gpu_profiles": {
                "RTX_50": { "sage": "v220_cu13", "kernels": ["nunchaku_cu13", "gguf", "flash_mla"] },
                "RTX_99": { "sage": "v999", "kernels": ["gguf"] }
            }
        });
        let w = validate_setup_config_shape(&cfg);
        assert!(w.iter().any(|m| m.contains("flash_mla")), "got {w:?}");
        assert!(w.iter().any(|m| m.contains("nunchaku_cu13")), "got {w:?}");
        assert!(w.iter().any(|m| m.contains("v999")), "got {w:?}");
        // Current upstream shape is clean.
        let clean = serde_json::json!({
            "components": {
                "kernels": {
                    "gguf": { "cmd": { "win": "https://x/gguf.whl" } },
                    "nunchaku_cu13": { "cmd": { "win": "https://x/n.whl" } }
                },
                "sage": { "v220_cu13": { "cmd": { "win": "https://x/s.whl" } } }
            },
            "gpu_profiles": {
                "RTX_50": { "sage": "v220_cu13", "kernels": ["nunchaku_cu13", "gguf"] }
            }
        });
        assert!(validate_setup_config_shape(&clean).is_empty());
    }
}
#[cfg(test)]
mod version_cmp_tests {
    use super::{gguf_wheel_version, parse_pip_show_versions, version_gt, wanted_triton_pin};
    #[test]
    fn gguf_version_cuts_percent_encoded_build_tag() {
        // Upstream percent-encodes `+` as `%2B` — the floor math must see
        // "1.0.23", not "1.0.23%2Btorch210cu130py311".
        assert_eq!(
            gguf_wheel_version(
                "https://github.com/deepbeepmeep/kernels/releases/download/gguf-v1.0.23/llamacpp_gguf_cuda-1.0.23%2Btorch210cu130py311-cp311-cp311-win_amd64.whl"
            )
            .as_deref(),
            Some("1.0.23")
        );
        assert_eq!(
            gguf_wheel_version("--no-deps https://x/llamacpp_gguf_cuda-1.0.23%2Btorch271cu128py310-cp310-cp310-win_amd64.whl")
                .as_deref(),
            Some("1.0.23")
        );
    }
    #[test]
    fn dotted_compare() {
        assert!(version_gt("1.0.21", "1.0.2"));
        assert!(!version_gt("1.0.2", "1.0.21"));
        assert!(version_gt("3.8.0", "3.3"));
        assert!(version_gt("3.3.1", "3.3"));
        assert!(!version_gt("1.0.21", "1.0.21"));
        assert!(!version_gt("3.3", "3.8.0"));
        assert!(!version_gt("", "1.0.2"));
    }
    #[test]
    fn gguf_version_extract() {
        assert_eq!(
            gguf_wheel_version(
                "https://x/llamacpp_gguf_cuda-1.0.14+torch210cu130py311-cp311-cp311-win_amd64.whl"
            )
            .as_deref(),
            Some("1.0.14")
        );
        assert_eq!(
            gguf_wheel_version(
                "https://x/llamacpp_gguf_cuda-1.0.2+torch210cu130py311-cp311-cp311-win_amd64.whl"
            )
            .as_deref(),
            Some("1.0.2")
        );
        assert_eq!(gguf_wheel_version("https://github.com/nunchaku-ai/nunchaku/releases/download/v1.2.1/nunchaku-1.2.1+cu13.0torch2.10-cp311-cp311-win_amd64.whl"), None);
        assert_eq!(gguf_wheel_version("triton-windows"), None);
    }
    #[test]
    fn triton_pin_parse() {
        assert_eq!(
            wanted_triton_pin("triton-windows<3.3").as_deref(),
            Some("3.3")
        );
        assert_eq!(
            wanted_triton_pin("triton-windows==3.3.1").as_deref(),
            Some("3.3.1")
        );
        assert_eq!(
            wanted_triton_pin("triton-windows>=3.3,<4").as_deref(),
            Some("3.3")
        );
        // v18.18 v37/v38 shape: `>=floor,<=ceiling` two-bound spec — the
        // floor bound is the wanted pin (min binding parsed; post tags stop
        // the version scan cleanly at "post").
        assert_eq!(
            wanted_triton_pin("-U \"triton-windows>=3.7.0.post26,<=3.7.1.post27\"").as_deref(),
            Some("3.7.0")
        );
        assert_eq!(wanted_triton_pin("triton-windows"), None);
        assert_eq!(
            wanted_triton_pin("https://x/triton_windows-3.3.1-cp311-win_amd64.whl"),
            None
        );
    }
    #[test]
    fn pip_show_max_wins() {
        let text = "Name: triton-windows\nVersion: 3.8.0\n---\nName: triton\nVersion: 3.7.1\n";
        assert_eq!(parse_pip_show_versions(text).as_deref(), Some("3.8.0"));
        assert_eq!(parse_pip_show_versions("ERROR: Package(s) not found"), None);
        assert_eq!(parse_pip_show_versions(""), None);
    }
}
pub(crate) fn build_install_plan(hw: &serde_json::Value) -> serde_json::Value {
    let vendor = hw
        .get("vendor")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN")
        .to_uppercase();
    let name = hw
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let vram = hw
        .get("vramMB")
        .and_then(|v| v.as_str())
        .unwrap_or("0")
        .split_whitespace()
        .next()
        .unwrap_or("0")
        .parse::<f64>()
        .unwrap_or(0.0);
    let driver = hw
        .get("driverVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let vram_gb = vram / 1024.0_f64;
    let is_gtx = name.to_uppercase().contains("GTX 10")
        || name.to_uppercase().contains("GTX 16")
        || (name.contains("10")
            && vendor == "NVIDIA"
            && (name.contains("1050") || name.contains("1060") || name.contains("1650")));
    let (cuda, torch, warn) = if vendor == "NVIDIA" {
        if is_gtx {
            ("CUDA 12.8", "PyTorch 2.7.1", String::new())
        } else {
            let mut w = String::new();
            if let Ok(dv) = driver.parse::<f64>() {
                if dv < 580.0 {
                    w = format!("NVIDIA driver {driver} < R580 — cu130 needs R580+");
                }
            }
            ("CUDA 13 (cu130)", "PyTorch 2.10", w)
        }
    } else if vendor == "AMD" {
        (
            "ROCm 10 (TheRock, default)",
            "PyTorch 2.13 (ROCm 10; HIP 7.14 opt-in)",
            String::new(),
        )
    } else if vendor == "APPLE" {
        ("MPS (Metal)", "PyTorch (MPS)", String::new())
    } else {
        ("CPU", "PyTorch (CPU)", String::new())
    };
    let _ = vram_gb;
    let _ = warn.clone();
    serde_json::json!({"vendor": vendor, "gpuName": name, "vramGb": vram, "cuda": cuda, "torch": torch, "driverWarning": warn, "profile": kernel_profile_key(&vendor, &name)})
}
#[tauri::command]
pub fn detect_gpus() -> serde_json::Value {
    let (cards, raw) = probe_nv_cards();
    if !cards.is_empty() {
        let gpus: Vec<serde_json::Value> = cards
            .iter()
            .map(|c| {
                serde_json::json!({
                    "index": c.index,
                    "uuid": c.uuid,
                    "name": c.name,
                    "vramMB": c.vram_mb as f64,
                    "freeMB": c.free_mb as f64,
                    "vendor": "NVIDIA",
                })
            })
            .collect();
        return serde_json::Value::Array(gpus);
    }
    // Old driver without the uuid column: one entry from the classic query,
    // never a bogus "Unknown" row that hides a working card.
    if let Some((name, mb, _driver)) = pick_smi_row(&raw) {
        return serde_json::json!([{"index": 0, "name": name, "vramMB": mb as f64, "vendor": "NVIDIA"}]);
    }
    // WMI fallback for AMD/Intel (Electron queryGpuList parity, dropped in port).
    if let Some((name, vendor, raw, _driver)) = wmi_gpu_fallback() {
        // 64-bit registry VRAM first — exact on 32GB cards (R9700 shows 32768).
        if let Some(mb) = wmi_dedicated_vram_mb(&name) {
            return serde_json::json!([{"index": 0, "name": name, "vramMB": mb as f64, "vendor": vendor}]);
        }
        // AdapterRAM caps at ~4GB (0/0xFFFFFFFF on big cards) — never truth.
        // Known-card table before giving up (same fallback as detect path).
        match adapter_ram_known_mb(raw) {
            Some(mb) => {
                return serde_json::json!([{"index": 0, "name": name, "vramMB": mb, "vendor": vendor}])
            }
            None => match known_vram_mb(&name) {
                Some(mb) => {
                    return serde_json::json!([{"index": 0, "name": name, "vramMB": mb as f64, "vendor": vendor}])
                }
                None => {
                    return serde_json::json!([{"index": 0, "name": format!("{name} (VRAM unknown)"), "vramMB": 0.0, "vendor": vendor}])
                }
            },
        }
    }
    // fallback single unknown
    serde_json::json!([{"index": 0, "name": "Unknown", "vramMB": 0, "vendor": "UNKNOWN"}])
}

#[tauri::command]
pub fn detect_hardware() -> serde_json::Value {
    // Describe the card the launcher pins, not nvidia-smi's first row: on a
    // two-GPU box the install overview used to size itself from the card
    // generation never runs on.
    let card = get_gpu_info_sync();
    let gpu_name = card
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("—")
        .to_string();
    let vram = card
        .get("vramMB")
        .and_then(|v| v.as_str())
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let vram_str = if vram > 0.0 {
        format!("{} MB", vram as i64)
    } else {
        "—".into()
    };
    // CPU/RAM via sysinfo — ~0ms vs 800ms powershell; brand string is equivalent to WMI Name
    let (cpu, ram) = {
        let mut sys = SYSINFO_CACHE.lock().unwrap();
        // refresh only if stale (>4s) to avoid re-allocating every detect_hardware call
        sys.refresh_memory();
        sys.refresh_cpu_all();
        let total = sys.total_memory();
        let ram_s = if total > 0 {
            format!("{} GB", total / 1073741824)
        } else {
            "—".into()
        };
        let cpu_s = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::var("PROCESSOR_IDENTIFIER").unwrap_or("—".into()));
        // If brand already contains GHz, don't append; else append frequency hint from sysinfo
        let cpu_s = if cpu_s.contains("GHz") || cpu_s.contains("MHz") {
            cpu_s
        } else {
            let freq = sys.cpus().first().map_or(0, sysinfo::Cpu::frequency);
            if freq > 0 {
                format!("{} ({:.2} GHz)", cpu_s, freq as f64 / 1000.0)
            } else {
                cpu_s
            }
        };
        (cpu_s, ram_s)
    };
    serde_json::json!({"cpu": cpu, "ram": ram, "gpu": gpu_name, "vram": vram_str})
}

/// Friendly component labels (mirrors Electron COMP_LABEL — same strings).
fn comp_label(code: &str) -> String {
    match code {
        "cu128" => "PyTorch 2.7.1 + CUDA 12.8".into(),
        "cu130" => "PyTorch 2.10.0 + CUDA 13.0".into(),
        // Key is rocm10 (upstream setup_config.json schema since the
        // ROCm 10 refresh); rocm65 stays as a fallback label for old checkouts.
        "rocm10" => "PyTorch 2.13 + ROCm 10 TheRock (HIP 7.14 opt-in)".into(),
        "rocm65" => "PyTorch 2.12 + ROCm 7.15 TheRock (HIP 7.14 opt-in)".into(),
        "mps" => "PyTorch (MPS)".into(),
        // Triton / flash / sage / sparge labels mirror upstream
        // setup_config.json `components.*.label` — the overview must not
        // invent pins. Both the short display value and the raw upstream
        // key are accepted (raw keys stay for forward compatibility).
        // Triton: upstream WF18.18 (fde3c7e) renamed the raw keys — the old
        // `v37` (3.6 spec) is now `v36`, and `v37` itself became the
        // recommended Triton 3.7 line (`>=3.7.0.post26,<=3.7.1.post27`,
        // Windows) that ships WanGP's prebuilt CUDA helper. `v38` (AMD) pins
        // the same range. See triton_label for the context-dependent labels.
        "3.2.x" | "v33" => "Triton 3.2.x (RTX 20XX only)".into(),
        "3.3.x" | "v34" => "Triton 3.3.x (Torch 2.7, RTX 30XX+)".into(),
        "3.6.x" | "v36" => "Triton 3.6.x (Torch 2.10, RTX 30XX+)".into(),
        "3.7.x" | "v38" => "Triton 3.7.x (Torch 2.13, AMD ROCm)".into(),
        "latest" => "Triton (latest)".into(),
        "1.0.6" | "v1" => "Sage Attention 1.0.6".into(),
        "2.1.1" | "v211" => "Sage Attention 2.1.1".into(),
        "2.2.0" | "v220" | "v220_cu13" => "Sage Attention 2.2.0 (CUDA 13)".into(),
        "0.1.0" | "v010_cu13" => "Sparge Attention 0.1.0 (CUDA 13)".into(),
        "v010_cu128" => "Sparge 0.1.0 (CUDA 12.8)".into(),
        "2.8.3" | "v210" => "Flash Attention 2.8.3".into(),
        "2.8.4 (ROCm)" | "v284_rocm" => "Flash Attention 2.8.4 (AMD ROCm / Triton)".into(),
        other => other.to_string(),
    }
}

/// Short version for an upstream `setup_config.json` component tag
/// (`v010_cu13` → `0.1.0`). Used where a panel shows the *expected* value next
/// to an installed one — the raw tag would read as noise in a spec row.
pub(crate) fn comp_short(code: &str) -> String {
    match code {
        "v1" => "1.0.6".into(),
        "v211" => "2.1.1".into(),
        "v220" | "v220_cu13" => "2.2.0".into(),
        "v010_cu128" | "v010_cu13" => "0.1.0".into(),
        "v210" => "2.8.3".into(),
        "v284_rocm" => "2.8.4".into(),
        "v33" => "3.2.x".into(),
        "v34" => "3.3.x".into(),
        // v18.18 renamed the triton keys: v36 = Triton 3.6.x, v37 = the
        // recommended Triton 3.7 Windows line (prebuilt helper builds).
        "v36" => "3.6.x".into(),
        "v37" => "3.7.x".into(),
        "v38" => "3.7.x".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod comp_short_tests {
    use super::comp_short;
    #[test]
    fn upstream_component_tags_map_to_versions() {
        // Raw setup_config.json tags must never leak into a panel.
        assert_eq!(comp_short("v010_cu13"), "0.1.0");
        assert_eq!(comp_short("v010_cu128"), "0.1.0");
        assert_eq!(comp_short("v210"), "2.8.3");
        assert_eq!(comp_short("v284_rocm"), "2.8.4");
        assert_eq!(comp_short("v220_cu13"), "2.2.0");
        assert_eq!(comp_short("v1"), "1.0.6");
        // v18.18 key rename: v37 is the Triton 3.7 line now, v36 is 3.6.
        assert_eq!(comp_short("v36"), "3.6.x");
        assert_eq!(comp_short("v37"), "3.7.x");
        assert_eq!(comp_short("v38"), "3.7.x");
        // Unknown values pass through unchanged (no silent data loss).
        assert_eq!(comp_short("sdpa"), "sdpa");
    }
}

/// Triton detail-row label. The 3.7 line exists on two stacks with different
/// upstream keys (`v37` = Torch 2.10 RTX 30XX+ Windows, `v38` = Torch 2.13 AMD
/// ROCm), so the context-free comp_label can't tell them apart — resolve
/// through the profile. Raw tags pass through for forward compatibility.
fn triton_label(profile_str: &str, code: &str) -> String {
    match code {
        "v37" => "Triton 3.7 Windows / 3.6 Linux (Torch 2.10, RTX 30XX+, Recommended)".into(),
        "v38" => "Triton 3.7.x (Torch 2.13, AMD ROCm)".into(),
        "3.7.x" => {
            if profile_str == "AMD" {
                "Triton 3.7.x (Torch 2.13, AMD ROCm)".into()
            } else {
                "Triton 3.7 Windows / 3.6 Linux (Torch 2.10, RTX 30XX+, Recommended)".into()
            }
        }
        other => comp_label(other),
    }
}

/// Kernel display names (mirrors Electron KERNEL_DISPLAY).
fn kernel_display(key: &str) -> (&str, &str) {
    match key {
        "nunchaku" | "nunchaku_cu13" => ("Nunchaku", "nunchaku"),
        "gguf" | "gguf_cu128" | "gguf_rocm10" | "llamacpp_gguf_cuda" => {
            ("GGUF (llamacpp)", "llamacpp_gguf_cuda")
        }
        "lightx2v" | "light2xv" | "lightx2v_kernel" => ("LightX2V", "lightx2v_kernel"),
        "sageattention" => ("SageAttention", "sageattention"),
        "spas_sage_attn" => ("Sparge (Sage)", "spas_sage_attn"),
        "flash_attn" => ("FlashAttention", "flash_attn"),
        "wgpkernels" | "wgpkernels_cu128" | "wgpkernels_rocm10" => {
            ("WGPKernels (INT8)", "wgpkernels")
        }
        "bitsandbytes" => ("bitsandbytes NF4", "bitsandbytes"),
        _ => (key, key),
    }
}

#[tauri::command]
pub fn get_hardware_profile() -> serde_json::Value {
    // Full per-profile version matrix lives in
    // hardware_profile_detail_with_ram (pure + unit-tested); this command
    // only probes hardware and delegates.
    // Same card the launcher pins: a multi-GPU box must not describe (or size)
    // the profile from a card generation will not run on.
    let gpu = get_gpu_info_sync();
    let vram_mb = gpu
        .get("vramMB")
        .and_then(|v| v.as_str())
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    hardware_profile_detail_with_ram(
        gpu.get("vendor")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN"),
        gpu.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        vram_mb,
        probe_ram_gb(),
    )
}

/// Best-effort total-RAM probe (GB) for the overview pid. Integer bytes
/// via powershell on Windows (same source as upstream setup.py
/// get_system_specs); None when the probe fails so callers tier
/// conservatively instead of on a lie.
#[cfg(windows)]
pub(crate) fn probe_ram_gb() -> Option<f64> {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let out = probe_command("POWERSHELL", "powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.bytes().all(|b| b.is_ascii_digit()) {
        let n: f64 = s.parse().ok()?;
        if n.is_finite() && n > 0.0 {
            return Some(n / GB);
        }
        return None;
    }
    // Legacy decimal-GB fallback ("79,8" / "79.8").
    if s.matches(['.', ',']).count() == 1 {
        let norm: String = s.chars().map(|c| if c == ',' { '.' } else { c }).collect();
        let gb: f64 = norm.parse().ok()?;
        if gb.is_finite() && gb > 0.0 {
            return Some(gb);
        }
    }
    None
}

#[cfg(not(windows))]
pub(crate) fn probe_ram_gb() -> Option<f64> {
    None
}

/// Pure profile matrix behind get_hardware_profile (no hardware probes —
/// unit-testable). INTEL_XPU maps to a CPU-only, kernel-free INTEL_CPU row:
/// neither upstream nor this launcher ships an XPU backend, so the overview
/// must not promise CUDA wheels (the old `_` fallthrough did). Install,
/// launch and smoke paths are untouched — Intel boxes keep working exactly
/// as before (CPU torch), only the labels are honest now.
/// The `profileNum` pid follows upstream setup.py `create_wgp_config`
/// thresholds (ram>60/>30, vram>22/>11) so the overview never contradicts an
/// Auto-Tune / install recommendation the way the old VRAM-only `pnum` did
/// (24GB VRAM + 8GB RAM showed P1, which needs 64GB). `ram_gb=None`
/// (unknown) stays conservative: never P1–P3, matching upstream-with-16GB-RAM
/// behavior.
pub(crate) fn hardware_profile_detail_with_ram(
    vendor: &str,
    name: &str,
    vram_mb: f64,
    ram_gb: Option<f64>,
) -> serde_json::Value {
    struct Prof {
        python: &'static str,
        torch: &'static str,
        triton: Option<&'static str>,
        sage: Option<&'static str>,
        sparge: Option<&'static str>,
        flash: Option<&'static str>,
        kernels: &'static [&'static str],
    }
    let vram_gb = vram_mb / 1024.0;
    let key = kernel_profile_key(vendor, name);
    let (profile_str, prof) = match key.as_str() {
        "GTX_10" => (
            "GTX_10",
            Prof {
                python: "3.10.9",
                torch: "2.7.1 CU12.8",
                triton: None,
                sage: None,
                sparge: None,
                flash: None,
                kernels: &[],
            },
        ),
        "RTX_20" => (
            "RTX_20",
            Prof {
                python: "3.11.14",
                torch: "2.10.0 CU13",
                triton: Some("3.2.x"),
                sage: Some("1.0.6"),
                sparge: None,
                // Upstream RTX_20 declares no flash attention (Turing has no
                // prebuilt wheel in setup_config.json) — don't promise it.
                // v18.20: upstream puts `wgpkernels` in this profile too
                // (WGPKernels 1.3.0 supports RTX 20XX and up).
                flash: None,
                kernels: &["wgpkernels", "nunchaku_cu13", "gguf"],
            },
        ),
        "RTX_30" => (
            "RTX_30",
            Prof {
                python: "3.11.14",
                torch: "2.10.0 CU13",
                // v18.18 (fde3c7e): recommended line is Triton 3.7 Windows
                // (`>=3.7.0.post26,<=3.7.1.post27`) — carries WanGP's prebuilt
                // CUDA helper, so the JIT-AV exposure is gone on these stacks.
                triton: Some("3.7.x"),
                sage: Some("2.2.0"),
                sparge: Some("0.1.0"),
                flash: Some("2.8.3"),
                kernels: &["wgpkernels", "nunchaku_cu13", "gguf"],
            },
        ),
        "RTX_40" => (
            "RTX_40",
            Prof {
                python: "3.11.14",
                torch: "2.10.0 CU13",
                // v18.18: Triton 3.7 Windows line (see RTX_30).
                triton: Some("3.7.x"),
                sage: Some("2.2.0"),
                sparge: Some("0.1.0"),
                flash: Some("2.8.3"),
                kernels: &["wgpkernels", "nunchaku_cu13", "gguf"],
            },
        ),
        "RTX_50" => (
            "RTX_50",
            Prof {
                python: "3.11.14",
                torch: "2.10.0 CU13",
                // v18.18: Triton 3.7 Windows line (see RTX_30).
                triton: Some("3.7.x"),
                sage: Some("2.2.0"),
                sparge: Some("0.1.0"),
                flash: Some("2.8.3"),
                kernels: &["wgpkernels", "nunchaku_cu13", "light2xv", "gguf"],
            },
        ),
        "MPS" => (
            "MPS",
            Prof {
                python: "3.11.14",
                torch: "MPS",
                triton: None,
                sage: None,
                sparge: None,
                flash: None,
                kernels: &[],
            },
        ),
        k if k.starts_with("AMD") => (
            "AMD",
            Prof {
                python: "3.12.10",
                torch: "ROCm 10 (HIP 7.14 opt-in)",
                // v18.18: AMD's triton-windows pins the same 3.7 line as
                // NVIDIA (`>=3.7.0.post26,<=3.7.1.post27`); the prebuilt HIP
                // helper engages automatically on Python 3.12 + torch 2.13
                // rocm envs (experimental pending AV validation).
                triton: Some("3.7.x"),
                sage: Some("1.0.6"),
                sparge: None,
                // ROCm has a prebuilt Triton-based flash wheel (upstream
                // `flash: v284_rocm`) — surface it instead of a dash.
                flash: Some("2.8.4 (ROCm)"),
                kernels: &["wgpkernels"],
            },
        ),
        // Intel → CPU torch, no kernels: XPU acceleration is not possible
        // (no upstream XPU backend exists). Display key is INTEL_CPU.
        "INTEL_XPU" => (
            "INTEL_CPU",
            Prof {
                python: "3.11.14",
                torch: "CPU",
                triton: None,
                sage: None,
                sparge: None,
                flash: None,
                kernels: &[],
            },
        ),
        _ => (
            key.as_str(),
            Prof {
                python: "3.11.14",
                torch: "2.10.0 CU13",
                // v18.18: Triton 3.7 Windows line (see RTX_30).
                triton: Some("3.7.x"),
                sage: Some("2.2.0"),
                sparge: Some("0.1.0"),
                flash: Some("2.8.3"),
                kernels: &["wgpkernels", "nunchaku_cu13", "gguf"],
            },
        ),
    };
    // Package chips with versions (Electron order + emoji).
    let mut packages: Vec<String> = Vec::new();
    packages.push(format!("🐍 Python {}", prof.python));
    packages.push(format!("🔥 PyTorch {}", prof.torch));
    if let Some(t) = prof.triton {
        packages.push(format!("⚡ Triton ({t})"));
    }
    if let Some(s) = prof.sage {
        packages.push(format!("🌀 Sage Attn {s}"));
    }
    if let Some(s) = prof.sparge {
        packages.push(format!("🌊 Sparge Attn {s}"));
    }
    if let Some(f) = prof.flash {
        packages.push(format!("💥 Flash Attn {f}"));
    }
    packages.push("📋 100+ reqs (diffusers, transformers, gradio, torchcodec…)".into());
    let kernel_labels: Vec<String> = prof
        .kernels
        .iter()
        .map(|k| kernel_display(k).0.to_string())
        .collect();
    let detail = serde_json::json!({
        "profile": profile_str,
        "python": prof.python,
        "torch": prof.torch,
        "triton": prof.triton.map(|t| triton_label(profile_str, t)).unwrap_or("—".into()),
        "sage": prof.sage.map(comp_label).unwrap_or("—".into()),
        "sparge": prof.sparge.map(comp_label).unwrap_or("—".into()),
        "flash": prof.flash.map(comp_label).unwrap_or("—".into()),
        "kernels": kernel_labels,
    });
    // Upstream-parity pid (setup.py create_wgp_config): RAM and VRAM
    // together. Unknown RAM stays conservative — never P1-P3.
    let pnum = match ram_gb {
        Some(ram) => {
            let high_ram = ram > 60.0;
            let mid_ram = ram > 30.0;
            let huge_vram = vram_gb > 22.0;
            let high_vram = vram_gb > 11.0;
            if high_ram && huge_vram {
                1
            } else if high_ram {
                2
            } else if mid_ram && huge_vram {
                3
            } else if mid_ram && high_vram {
                4
            } else {
                5
            }
        }
        None => {
            if vram_gb > 11.0 {
                4
            } else {
                5
            }
        }
    };
    serde_json::json!({
        "profile": profile_str,
        "vramGb": vram_gb,
        "profileNum": pnum,
        "detail": detail,
        "packages": packages,
        "kernels": prof.kernels.iter().map(|k| { let (label, dist) = kernel_display(k); serde_json::json!({"label": label, "dist": dist}) }).collect::<Vec<_>>(),
        "kernelsRaw": prof.kernels
    })
}

static LAST_NVIDIA: LazyLock<Mutex<Option<serde_json::Value>>> = LazyLock::new(|| Mutex::new(None));
/// Registry VRAM lookup memoized per display name: the metrics tile asked
/// powershell for the driver key on **every** tick (1-2 s) even though VRAM
/// never changes. `None` is cached too, so a card without a QWORD stops
/// re-probing as well.
type VramCache = Option<(String, Option<u64>)>;
static CACHED_VRAM_MB: LazyLock<Mutex<VramCache>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn cached_dedicated_vram_mb(display_name: &str) -> Option<u64> {
    if let Ok(g) = CACHED_VRAM_MB.lock() {
        if let Some((n, v)) = g.as_ref() {
            if n == display_name {
                return *v;
            }
        }
    }
    let v = wmi_dedicated_vram_mb(display_name);
    if let Ok(mut g) = CACHED_VRAM_MB.lock() {
        *g = Some((display_name.to_string(), v));
    }
    v
}

pub(crate) fn get_cached_igpu() -> Option<serde_json::Value> {
    // ponytail: WMI probed once, cached forever — was running powershell every 2s in hot loop
    let m = &CACHED_IGPU;
    if let Ok(g) = m.lock() {
        if let Some(v) = g.clone() {
            return Some(v);
        }
    }
    // first call: run WMI, cache result (even None as explicit)
    let mut igpu: Option<serde_json::Value> = None;
    #[cfg(windows)]
    {
        if let Ok(wmi_out) = probe_command("POWERSHELL", "powershell").args(["-NoProfile","-Command","Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM | ForEach-Object { $_.Name + '|' + $_.AdapterRAM }"]).output() {
            if wmi_out.status.success() {
                let wmi_s = String::from_utf8_lossy(&wmi_out.stdout).trim().to_string();
                for ln in wmi_s.lines() {
                    if let Some((n, r)) = ln.split_once('|') {
                        let name = n.trim();
                        if name.is_empty() || name.to_lowercase().contains("nvidia") { continue; }
                        let lower = name.to_lowercase();
                        if lower.contains("intel") || lower.contains("amd") || lower.contains("radeon") || lower.contains("arc") {
                            let raw_mb = r.trim().parse::<u64>().unwrap_or(0);
                            // Same 4GB-cap guard as detection (R9700 metrics tile
                            // showed a bogus 4095 MB before this).
                            let vram_mb = adapter_ram_known_mb(raw_mb).map(|mb| mb as i64).unwrap_or(0);
                            let fmt2 = if vram_mb>0 { format!("{vram_mb} MB") } else { "—".into() };
                            igpu = Some(serde_json::json!({"name": name, "vram": fmt2}));
                            break;
                        }
                    }
                }
            }
        }
    }
    // cache sentinel: if no igpu found, store Null so we don't re-probe
    if let Ok(mut g) = m.lock() {
        *g = igpu.clone().or(Some(serde_json::Value::Null));
    }
    igpu
}
#[tauri::command]
pub fn get_system_metrics() -> serde_json::Value {
    // throttle: if called <1.2s ago, return cached metrics (prevents double-fire from dashboard+polling)
    if let Ok(g) = METRICS_CACHE.lock() {
        if let Some((t, v)) = g.as_ref() {
            if t.elapsed() < std::time::Duration::from_millis(1200) {
                return v.clone();
            }
        }
    }
    let mut result = serde_json::json!({"ramFree": null, "vramFree": null, "cpu": null, "gpu": null, "ramUsed": null, "ramTotal": null, "vramUsed": null, "vramTotal": null, "ram": null, "vram": null, "gpus": [], "gpu2": null, "vram2": null, "vramFree2": null, "vramUsed2": null, "vramTotal2": null});
    // RAM/CPU via reused sysinfo instance (no alloc per tick)
    {
        let mut sys = SYSINFO_CACHE.lock().unwrap();
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        let total = sys.total_memory();
        let free = sys.free_memory();
        let used = sys.used_memory();
        let gb = |b: u64| format!("{} GB", b / 1073741824);
        result["ramFree"] = serde_json::Value::String(gb(free));
        result["ramTotal"] = serde_json::Value::String(gb(total));
        result["ramUsed"] = serde_json::Value::String(gb(used));
        if total > 0 {
            result["ram"] = serde_json::json!((used as f64 / total as f64 * 100.0).round() as i64);
        }
        // Always publish it: a real 0% reading used to be dropped, leaving the
        // topbar showing the previous (stale) value.
        let cpu = sys.global_cpu_usage().round().max(0.0) as i64;
        result["cpu"] = serde_json::json!(cpu);
    }
    // nvidia-smi for VRAM/GPU — per-GPU breakdown + WMI iGPU fallback (mirrors Electron main.js)
    {
        let run = |args: [&str; 2]| {
            probe_command("NVIDIA_SMI", "nvidia-smi")
                .args(args)
                .output()
        };
        let six = run([
            "--query-gpu=index,uuid,memory.free,memory.used,memory.total,utilization.gpu",
            "--format=csv,noheader,nounits",
        ]);
        let good = |r: &std::io::Result<std::process::Output>| {
            r.as_ref().is_ok_and(|o| {
                o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty()
            })
        };
        let out = if good(&six) {
            six
        } else {
            // A driver that rejects `uuid` still gets a topbar from the classic
            // 4-field query; the parser below accepts both shapes.
            run([
                "--query-gpu=memory.free,memory.used,memory.total,utilization.gpu",
                "--format=csv,noheader,nounits",
            ])
        };
        if let Ok(o) = out {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !s.is_empty() {
                    let fmt_mb = |mb: i64| {
                        if mb >= 1024 {
                            format!("{} GB", (mb as f64 / 1024.0).round() as i64)
                        } else {
                            format!("{mb} MB")
                        }
                    };
                    let mut per_gpu: Vec<(i32, String, serde_json::Value)> = Vec::new();
                    for (pos, line) in s.lines().enumerate() {
                        let p: Vec<&str> = line.split(',').map(str::trim).collect();
                        // 6 fields = index,uuid,…; 4 = the legacy form.
                        let (idx, uuid, f, u, t, g) = if p.len() >= 6 {
                            (
                                p[0].parse::<i32>().unwrap_or(pos as i32),
                                p[1].to_string(),
                                p[2].parse::<i64>().unwrap_or(0),
                                p[3].parse::<i64>().unwrap_or(0),
                                p[4].parse::<i64>().unwrap_or(0),
                                p[5].parse::<i64>().unwrap_or(0),
                            )
                        } else if p.len() >= 4 {
                            (
                                pos as i32,
                                String::new(),
                                p[0].parse::<i64>().unwrap_or(0),
                                p[1].parse::<i64>().unwrap_or(0),
                                p[2].parse::<i64>().unwrap_or(0),
                                p[3].parse::<i64>().unwrap_or(0),
                            )
                        } else {
                            continue;
                        };
                        let vram_pct = if t > 0 {
                            (u as f64 / t as f64 * 100.0).round() as i64
                        } else {
                            0
                        };
                        per_gpu.push((
                            idx,
                            uuid,
                            serde_json::json!({"smi": idx, "free": f, "used": u, "total": t, "gpu": g, "vram": vram_pct, "vramFree": fmt_mb(f), "vramUsed": fmt_mb(u), "vramTotal": fmt_mb(t)}),
                        ));
                    }
                    // The topbar reports the card generation runs on, never a
                    // sum: summing let a full 24 GB card read "40 GB free"
                    // because an idle second device (or iGPU) was added in.
                    let pick = saved_gpu_pick();
                    let pick_uuid = pick.strip_prefix("uuid:").unwrap_or("");
                    let pick_ord = pick
                        .strip_prefix("cuda:")
                        .and_then(|n| n.parse::<i32>().ok());
                    let main = if pick.eq_ignore_ascii_case("all") {
                        per_gpu.first()
                    } else if !pick_uuid.is_empty() {
                        per_gpu
                            .iter()
                            .find(|(_, u, _)| u.eq_ignore_ascii_case(pick_uuid))
                    } else if let Some(n) = pick_ord {
                        per_gpu.iter().find(|(i, _, _)| *i == n)
                    } else {
                        None
                    }
                    .or_else(|| {
                        per_gpu
                            .iter()
                            .max_by_key(|(_, _, g)| g["total"].as_i64().unwrap_or(0))
                    });
                    if let Some((_, _, m)) = main {
                        result["vramFree"] = m["vramFree"].clone();
                        result["vramUsed"] = m["vramUsed"].clone();
                        result["vramTotal"] = m["vramTotal"].clone();
                        result["vram"] = m["vram"].clone();
                        result["gpu"] = m["gpu"].clone();
                    }
                    // build gpus array for topbar
                    let mut gpus_arr: Vec<serde_json::Value> = per_gpu.iter().enumerate().map(|(i,(_,_,g))| {
                    serde_json::json!({"index": i, "smi": g["smi"], "gpu": g["gpu"], "vram": g["vram"], "vramFree": g["vramFree"], "vramUsed": g["vramUsed"], "vramTotal": g["vramTotal"]})
                }).collect();
                    // iGPU fallback: cached once, not every 2s (was 400ms powershell in hot loop).
                    // Name-keyed tiles are WMI primaries (AMD/Intel-only box) — never append a dupe.
                    if gpus_arr.len() == 1 && gpus_arr[0].get("name").is_none() {
                        if let Some(igpu) = get_cached_igpu() {
                            if !igpu.is_null() {
                                let name = igpu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let fmt2 = igpu
                                    .get("vram")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("—")
                                    .to_string();
                                gpus_arr.push(serde_json::json!({"index": gpus_arr.len(), "gpu": 0, "vram": null, "vramFree": fmt2.clone(), "vramUsed": "0 MB", "vramTotal": fmt2, "name": name}));
                            }
                        }
                    }
                    result["gpus"] = serde_json::Value::Array(gpus_arr.clone());
                    if gpus_arr.len() > 1 {
                        if let Some(g2) = gpus_arr.get(1) {
                            result["gpu2"] =
                                g2.get("gpu").cloned().unwrap_or(serde_json::Value::Null);
                            result["vram2"] =
                                g2.get("vram").cloned().unwrap_or(serde_json::Value::Null);
                            result["vramFree2"] = g2
                                .get("vramFree")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            result["vramUsed2"] = g2
                                .get("vramUsed")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            result["vramTotal2"] = g2
                                .get("vramTotal")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                        }
                    }
                    LAST_NVIDIA.lock().unwrap().replace(result.clone());
                }
            } else if let Some(last) = LAST_NVIDIA.lock().ok().and_then(|g| g.clone()) {
                result["vramFree"] = last["vramFree"].clone();
                result["vramUsed"] = last["vramUsed"].clone();
                result["vramTotal"] = last["vramTotal"].clone();
                result["vram"] = last["vram"].clone();
                result["gpu"] = last["gpu"].clone();
                result["gpus"] = last["gpus"].clone();
                result["gpu2"] = last["gpu2"].clone();
                result["vram2"] = last["vram2"].clone();
                result["vramFree2"] = last["vramFree2"].clone();
                result["vramUsed2"] = last["vramUsed2"].clone();
                result["vramTotal2"] = last["vramTotal2"].clone();
            }
        }
    }
    // AMD/Intel-only box (no nvidia-smi output): primary tile from the cached WMI probe.
    // Total prefers the 64-bit registry VRAM (exact on 32GB cards) over AdapterRAM.
    if result["gpus"].as_array().is_none_or(|a| a.is_empty()) {
        if let Some(igpu) = get_cached_igpu() {
            if !igpu.is_null() {
                let name = igpu
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GPU")
                    .to_string();
                let total = cached_dedicated_vram_mb(&name)
                    .map(|mb| {
                        if mb >= 1024 {
                            format!("{} GB", (mb as f64 / 1024.0).round() as i64)
                        } else {
                            format!("{mb} MB")
                        }
                    })
                    .unwrap_or_else(|| {
                        igpu.get("vram")
                            .and_then(|v| v.as_str())
                            .unwrap_or("—")
                            .to_string()
                    });
                result["gpus"] = serde_json::json!([{"index": 0, "gpu": null, "vram": null, "vramFree": total.clone(), "vramUsed": "—", "vramTotal": total, "name": name}]);
            }
        }
    }
    // cache for throttle
    if let Ok(mut g) = METRICS_CACHE.lock() {
        *g = Some((std::time::Instant::now(), result.clone()));
    }
    result
}

/// Simulated AMD-box integration test (Windows): fakes nvidia-smi (absent)
/// and powershell (canned R9700 WMI + registry answers) via the WGP_PROBE_*
/// hook, then drives the REAL detection → plan → torch-URL chain.
/// Pattern: `set "ARGS=%*"` is parse-safe for | $ { }; dispatch by substring.
#[cfg(all(test, windows))]
mod amd_sim_tests {
    use super::*;
    use std::sync::Mutex;
    static SIM_LOCK: Mutex<()> = Mutex::new(());
    fn write_fakes(dir: &std::path::Path) {
        std::fs::write(dir.join("nvidia-smi.cmd"), "@echo off\r\nexit /b 1\r\n").unwrap();
        let ps = "@echo off\r\nif \"%~1\"==\"--\" goto dispatch\r\nexit /b 1\r\n:dispatch\r\nif \"%~3\"==\"-Command\" goto powershell\r\nexit /b 1\r\n:powershell\r\nset \"Q=%~4\"\r\nif not \"%Q:Win32_VideoController=%\"==\"%Q%\" goto wmi\r\nif not \"%Q:HardwareInformation=%\"==\"%Q%\" goto reg\r\nexit /b 1\r\n:wmi\r\necho AMD Radeon AI PRO R9700^|0^|32.0.11029.1008\r\necho AMD Radeon Graphics^|0^|32.0.11029.1008\r\nexit /b 0\r\n:reg\r\nif defined WGP_SIM_NO_REG exit /b 1\r\necho AMD Radeon AI PRO R9700^|34359738368\r\nexit /b 0\r\n";
        std::fs::write(dir.join("powershell.cmd"), ps).unwrap();
    }
    #[test]
    fn simulated_dual_nvidia_pins_the_picked_card() {
        let _guard = SIM_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("wgp-sim-nv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Two cards in nvidia-smi (PCI) order: index 0 is the small one.
        std::fs::write(
            dir.join("nvidia-smi.cmd"),
            "@echo off\r\necho 0, GPU-00000000-0000-0000-0000-000000000001, NVIDIA GeForce RTX 4070, 8188, 7000, 566.03\r\necho 1, GPU-00000000-0000-0000-0000-000000000002, NVIDIA GeForce RTX 4090, 24564, 900, 566.03\r\nexit /b 0\r\n",
        )
        .unwrap();
        let old = std::env::var("WGP_PROBE_NVIDIA_SMI").ok();
        std::env::set_var(
            "WGP_PROBE_NVIDIA_SMI",
            dir.join("nvidia-smi.cmd").to_string_lossy().to_string(),
        );
        let gpus = detect_gpus();
        let g = gpus.as_array().unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0]["index"], serde_json::json!(0));
        assert_eq!(
            g[0]["uuid"],
            serde_json::json!("GPU-00000000-0000-0000-0000-000000000001")
        );
        assert_eq!(g[0]["vramMB"], serde_json::json!(8188.0));
        assert_eq!(g[1]["vramMB"], serde_json::json!(24564.0));
        // Auto pins the biggest card (cuda:1 in nvidia-smi order).
        let cards = nv_cards();
        let pin = resolve_gpu_pick("auto", &cards);
        assert_eq!(pin.arg.as_deref(), Some("cuda:1"));
        assert_eq!(pin.card.as_ref().unwrap().vram_mb, 24564);
        // A saved "cuda:0" means nvidia-smi card 0, even though CUDA's default
        // FASTEST_FIRST ordering would have put that name on another ordinal.
        let pin = resolve_gpu_pick("cuda:0", &cards);
        assert_eq!(pin.arg.as_deref(), Some("cuda:0"));
        assert_eq!(pin.card.as_ref().unwrap().name, "NVIDIA GeForce RTX 4070");
        assert!(pin.note.is_none());
        match old {
            Some(v) => std::env::set_var("WGP_PROBE_NVIDIA_SMI", v),
            None => std::env::remove_var("WGP_PROBE_NVIDIA_SMI"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Driverless box: WMI reports only the Basic Display Adapter. Detection
    /// must surface that name (vendor "unknown") — filtering it as "unknown
    /// vendor" made the fatal preflight check unreachable.
    #[test]
    fn basic_display_adapter_is_detected() {
        let _guard = SIM_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("wgp-sim-bd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("nvidia-smi.cmd"), "@echo off\r\nexit /b 1\r\n").unwrap();
        let ps = "@echo off\r\nif \"%~1\"==\"--\" goto dispatch\r\nexit /b 1\r\n:dispatch\r\nif \"%~3\"==\"-Command\" goto powershell\r\nexit /b 1\r\n:powershell\r\nset \"Q=%~4\"\r\nif not \"%Q:Win32_VideoController=%\"==\"%Q%\" goto wmi\r\nexit /b 1\r\n:wmi\r\necho Microsoft Basic Display Adapter^|0^|10.0.19041.1\r\nexit /b 0\r\n";
        std::fs::write(dir.join("powershell.cmd"), ps).unwrap();
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{};{old_path}", dir.display()));
        std::env::set_var(
            "WGP_PROBE_NVIDIA_SMI",
            dir.join("nvidia-smi.cmd").to_string_lossy().to_string(),
        );
        std::env::set_var(
            "WGP_PROBE_POWERSHELL",
            dir.join("powershell.cmd").to_string_lossy().to_string(),
        );
        let info = get_gpu_info_sync();
        assert_eq!(info.get("vendor").and_then(|v| v.as_str()), Some("unknown"));
        assert!(
            info.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase()
                .contains("basic display"),
            "got {info}"
        );
        // The plan stays CPU (nothing to accelerate), never a fabricated GPU.
        assert_eq!(
            build_install_plan(&info)
                .get("profile")
                .and_then(|v| v.as_str()),
            Some("CPU")
        );
        std::env::remove_var("WGP_PROBE_NVIDIA_SMI");
        std::env::remove_var("WGP_PROBE_POWERSHELL");
        std::env::set_var("PATH", old_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn simulated_r9700_end_to_end() {
        let _guard = SIM_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("wgp-sim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_fakes(&dir);
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{};{old_path}", dir.display()));
        std::env::set_var(
            "WGP_PROBE_NVIDIA_SMI",
            dir.join("nvidia-smi.cmd").to_string_lossy().to_string(),
        );
        std::env::set_var(
            "WGP_PROBE_POWERSHELL",
            dir.join("powershell.cmd").to_string_lossy().to_string(),
        );
        std::env::remove_var("WGP_SIM_NO_REG");

        // detect → profile → install plan → torch URLs (AdapterRAM is 0: the
        // 32768 MB MUST come from the simulated registry QWORD).
        let gpus = detect_gpus();
        let g = &gpus.as_array().unwrap()[0];
        assert_eq!(g.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(
            g.get("name").and_then(|v| v.as_str()),
            Some("AMD Radeon AI PRO R9700")
        );
        assert_eq!(g.get("vramMB").and_then(|v| v.as_f64()), Some(32768.0));

        let info = get_gpu_info_sync();
        assert_eq!(info.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(
            info.get("vramMB").and_then(|v| v.as_str()),
            Some("32768 MiB")
        );
        // Driver version now flows through detection (preflight gate).
        assert_eq!(
            info.get("driverVersion").and_then(|v| v.as_str()),
            Some("32.0.11029.1008")
        );
        // dGPU + iGPU: both listed, dGPU first (selection unchanged).
        let all = wmi_all_gpus();
        assert_eq!(all.len(), 2);
        assert!(all[0].0.contains("R9700") && all[1].0.contains("Graphics"));

        let plan = build_install_plan(&info);
        assert_eq!(
            plan.get("profile").and_then(|v| v.as_str()),
            Some("AMD_GFX1201")
        );
        assert_eq!(
            plan.get("cuda").and_then(|v| v.as_str()),
            Some("ROCm 10 (TheRock, default)")
        );

        // Stable whl-next primary (pinned torch 2.13 + device extras),
        // nightly whl-next float fallback.
        let (primary, staging) =
            crate::install::amd_therock_torch_cmds("AMD_GFX1201", "AMD Radeon AI PRO R9700")
                .unwrap();
        assert!(
            primary.contains("stable.repo.amd.com/rocm/whl-next/"),
            "got {primary}"
        );
        assert!(primary.contains("2.13.0+rocm10.0.0"), "got {primary}");
        assert!(primary.contains("device-gfx1201"), "got {primary}");
        assert!(
            staging.contains("nightly.repo.amd.com/rocm/whl-next/"),
            "got {staging}"
        );

        // registry-absent hardware (the 0.5.1 reporter box: PRO driver
        // exposes no parseable QWORD): the known-card table still resolves
        // 32768 — honest unknown only for truly unknown names.
        std::env::set_var("WGP_SIM_NO_REG", "1");
        let gpus2 = detect_gpus();
        let g2 = &gpus2.as_array().unwrap()[0];
        assert_eq!(g2.get("vendor").and_then(|v| v.as_str()), Some("AMD"));
        assert_eq!(
            g2.get("name").and_then(|v| v.as_str()),
            Some("AMD Radeon AI PRO R9700")
        );
        assert_eq!(g2.get("vramMB").and_then(|v| v.as_f64()), Some(32768.0));

        std::env::remove_var("WGP_SIM_NO_REG");
        std::env::remove_var("WGP_PROBE_NVIDIA_SMI");
        std::env::remove_var("WGP_PROBE_POWERSHELL");
        std::env::set_var("PATH", old_path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod adapter_ram_tests {
    use super::{adapter_ram_known_mb, distinctive_tokens};
    #[test]
    fn cap_values_are_unknown() {
        // The R9700 report: 32GB card, AdapterRAM capped → was shown as 4GB.
        assert_eq!(adapter_ram_known_mb(0), None);
        assert_eq!(adapter_ram_known_mb(0xFFFF_FFFF), None);
        assert_eq!(adapter_ram_known_mb(4294967295), None);
        // Genuine small readings stay unknown for tiering honesty.
        assert_eq!(adapter_ram_known_mb(512 * 1024 * 1024), None);
        // Real sub-4GB readings pass through (3GB card, 2GB floor).
        assert_eq!(adapter_ram_known_mb(3221225472), Some(3072.0));
        assert_eq!(adapter_ram_known_mb(2147483648), Some(2048.0));
    }
    #[test]
    fn tokens_pick_model_numbers() {
        assert!(distinctive_tokens("AMD Radeon AI PRO R9700").contains(&"R9700".to_string()));
        assert!(distinctive_tokens("AMD Radeon RX 9070 XT").contains(&"9070".to_string()));
        // Generic iGPU names yield nothing (can never false-match).
        assert!(distinctive_tokens("AMD Radeon(TM) Graphics").is_empty());
    }
}
#[cfg(test)]
mod overview_pid_tests {
    use super::hardware_profile_detail_with_ram;
    fn pnum(vendor: &str, name: &str, vram_mb: f64, ram_gb: Option<f64>) -> i64 {
        hardware_profile_detail_with_ram(vendor, name, vram_mb, ram_gb)["profileNum"]
            .as_i64()
            .unwrap()
    }
    #[test]
    fn upstream_parity_pid() {
        // Mirrors setup.py create_wgp_config thresholds (ram>60/>30,
        // vram>22/>11).
        assert_eq!(
            pnum("AMD", "AMD Radeon RX 9070 XT", 16.0 * 1024.0, Some(80.0)),
            2
        );
        assert_eq!(
            pnum("AMD", "AMD Radeon RX 9070 XT", 16.0 * 1024.0, Some(32.0)),
            4
        );
        assert_eq!(
            pnum("AMD", "AMD Radeon RX 9060 XT", 16.0 * 1024.0, Some(16.0)),
            5
        );
        assert_eq!(
            pnum(
                "NVIDIA",
                "NVIDIA GeForce RTX 4090",
                24.0 * 1024.0,
                Some(80.0)
            ),
            1
        );
        assert_eq!(
            pnum(
                "NVIDIA",
                "NVIDIA GeForce RTX 4090",
                24.0 * 1024.0,
                Some(32.0)
            ),
            3
        );
    }
    #[test]
    fn unknown_ram_never_overpromises() {
        // The old VRAM-only pnum showed P1 for 24GB VRAM + 8GB RAM.
        // Unknown RAM caps at P4 (matches upstream-with-16GB-RAM).
        assert_eq!(pnum("NVIDIA", "X", 24.0 * 1024.0, None), 4);
        assert_eq!(pnum("AMD", "AMD Radeon RX 9060 XT", 16.0 * 1024.0, None), 4);
        assert_eq!(pnum("AMD", "X", 8.0 * 1024.0, None), 5);
    }
    #[test]
    fn overview_pins_match_upstream_setup_config() {
        // Sources of truth: setup_config.json gpu_profiles + components.*.label.
        // RTX_20 pins triton v33 (3.2.x) and declares no flash attention.
        let rtx20 = hardware_profile_detail_with_ram(
            "NVIDIA",
            "NVIDIA GeForce RTX 2080 Ti",
            11.0 * 1024.0,
            Some(32.0),
        );
        assert!(
            rtx20["detail"]["triton"]
                .as_str()
                .unwrap()
                .contains("3.2.x"),
            "RTX_20 triton: {}",
            rtx20["detail"]["triton"]
        );
        assert_eq!(rtx20["detail"]["flash"], serde_json::json!("—"));
        // RTX 30–50 pin the recommended Triton 3.7 Windows line (v18.18
        // fde3c7e — prebuilt helper builds) and flash 2.8.3.
        let rtx40 = hardware_profile_detail_with_ram(
            "NVIDIA",
            "NVIDIA GeForce RTX 4090",
            24.0 * 1024.0,
            Some(64.0),
        );
        assert!(
            rtx40["detail"]["triton"].as_str().unwrap().contains("3.7"),
            "RTX_40 triton: {}",
            rtx40["detail"]["triton"]
        );
        assert!(rtx40["detail"]["flash"].as_str().unwrap().contains("2.8.3"));
        // AMD pins triton v38 (3.7.x, Rocm line) and ships the ROCm flash wheel.
        let amd = hardware_profile_detail_with_ram(
            "AMD",
            "AMD Radeon RX 9070 XT",
            16.0 * 1024.0,
            Some(32.0),
        );
        assert_eq!(
            amd["detail"]["triton"],
            serde_json::json!("Triton 3.7.x (Torch 2.13, AMD ROCm)")
        );
        assert_eq!(
            amd["detail"]["flash"],
            serde_json::json!("Flash Attention 2.8.4 (AMD ROCm / Triton)")
        );
        // WGPKernels 1.3.0 is in the setup_config profiles — RTX 20/30/40/50
        // (upstream added Turing in v18.20) plus AMD RDNA4.
        let labels = |d: serde_json::Value| {
            d["kernels"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|k| k.get("label"))
                .filter_map(|l| l.as_str())
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        };
        assert!(labels(rtx40).contains(&"WGPKernels (INT8)".to_string()));
        assert!(labels(amd).contains(&"WGPKernels (INT8)".to_string()));
        let rtx20 = hardware_profile_detail_with_ram(
            "NVIDIA",
            "NVIDIA GeForce RTX 2080 Ti",
            11.0 * 1024.0,
            Some(32.0),
        );
        assert!(labels(rtx20).contains(&"WGPKernels (INT8)".to_string()));
    }
}
#[cfg(test)]
mod intel_cpu_tests {
    use super::{build_install_plan, hardware_profile_detail_with_ram, kernel_profile_key};
    #[test]
    fn intel_stays_cpu_honest() {
        // Keys stable (setup.py never sees them; install/launch behavior unchanged).
        assert_eq!(
            kernel_profile_key("INTEL", "Intel UHD Graphics 770"),
            "INTEL_XPU"
        );
        assert_eq!(
            kernel_profile_key("INTEL", "Intel Arc A770 Graphics"),
            "INTEL_XPU"
        );
        for n in ["Intel UHD Graphics 770", "Intel Arc A770 Graphics"] {
            // Install plan: CPU payload, no XPU promise anywhere.
            let plan = build_install_plan(
                &serde_json::json!({"vendor":"INTEL","name":n,"vramMB":"128 MiB","driverVersion":""}),
            );
            assert_eq!(plan["cuda"], serde_json::json!("CPU"));
            assert_eq!(plan["torch"], serde_json::json!("PyTorch (CPU)"));
            assert_eq!(plan["profile"], serde_json::json!("INTEL_XPU"));
            // Overview: INTEL_CPU row, kernel-free (old fallthrough promised CUDA wheels).
            let d = hardware_profile_detail_with_ram("INTEL", n, 0.0, None);
            assert_eq!(d["profile"], serde_json::json!("INTEL_CPU"));
            assert_eq!(d["kernelsRaw"].as_array().unwrap().len(), 0);
            assert!(d["detail"]["torch"].as_str().unwrap().contains("CPU"));
            assert!(!d["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p.as_str().unwrap().contains("CUDA")));
        }
    }
}
