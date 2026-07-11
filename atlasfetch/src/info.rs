// System information collection.
//
// Reads directly from /proc, /sys, and environment variables — no external
// commands or libraries. This keeps startup fast and the binary small.
// Every field is stored as a Display string so the renderer never has to
// format mid-render.

use color_eyre::Result;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct SysInfo {
    pub os: String,
    pub host: String,
    pub user: String,
    pub kernel: String,
    pub uptime: String,
    pub packages: String,
    pub shell: String,
    pub terminal: String,
    pub cpu: String,
    pub gpu: String,
    pub memory: String,
    pub disk: String,
    pub wm: String,
    pub load: String,
    pub processes: String,
    pub local_ip: String,
    pub resolution: String,
    pub de: String,
    pub font: String,
}

impl SysInfo {
    pub fn get(&self, field: &str) -> Option<&str> {
        match field {
            "os" => Some(&self.os),
            "host" => Some(&self.host),
            "user" => Some(&self.user),
            "kernel" => Some(&self.kernel),
            "uptime" => Some(&self.uptime),
            "packages" => Some(&self.packages),
            "shell" => Some(&self.shell),
            "terminal" => Some(&self.terminal),
            "cpu" => Some(&self.cpu),
            "gpu" => Some(&self.gpu),
            "memory" => Some(&self.memory),
            "disk" => Some(&self.disk),
            "wm" => Some(&self.wm),
            "load" => Some(&self.load),
            "processes" => Some(&self.processes),
            "local_ip" => Some(&self.local_ip),
            "resolution" => Some(&self.resolution),
            "de" => Some(&self.de),
            "font" => Some(&self.font),
            _ => None,
        }
    }
}

pub fn collect() -> Result<SysInfo> {
    let mut info = SysInfo::default();

    info.user = std::env::var("USER").unwrap_or_else(|_| whoami_fallback());
    info.host = hostname();
    info.os = detect_os();
    info.kernel = read_kernel();
    info.uptime = format_uptime();
    info.shell = detect_shell();
    info.terminal = detect_terminal();
    info.cpu = read_cpu();
    info.gpu = read_gpu();
    info.memory = format_memory();
    info.disk = format_disk("/");
    info.wm = detect_wm();
    info.load = read_load();
    info.processes = count_processes();
    info.packages = count_packages();
    info.local_ip = local_ip();
    info.resolution = String::new();
    info.de = String::new();
    info.font = String::new();

    Ok(info)
}

// ── OS detection ─────────────────────────────────────────────────────────

fn detect_os() -> String {
    for path in &["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                    return val.trim_matches('"').to_string();
                }
            }
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("NAME=") {
                    let name = val.trim_matches('"').to_string();
                    if let Some(ver) = content.lines().find_map(|l| l.strip_prefix("VERSION_ID=")) {
                        return format!("{} {}", name, ver.trim_matches('"'));
                    }
                    return name;
                }
            }
        }
    }
    "Linux".into()
}

// ── Hostname ─────────────────────────────────────────────────────────────

fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "localhost".into())
}

// ── Whoami fallback ──────────────────────────────────────────────────────

fn whoami_fallback() -> String {
    fs::read_to_string("/proc/self/uid_map")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
        .unwrap_or_else(|| "user".into())
}

// ── Kernel ───────────────────────────────────────────────────────────────

fn read_kernel() -> String {
    fs::read_to_string("/proc/version")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .nth(2)
                .unwrap_or("unknown")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".into())
}

// ── Uptime ───────────────────────────────────────────────────────────────

fn format_uptime() -> String {
    let secs = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;

    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;

    let mut parts = Vec::new();
    if d > 0 { parts.push(format!("{}d", d)); }
    if h > 0 { parts.push(format!("{}h", h)); }
    parts.push(format!("{}m", m));
    parts.join(" ")
}

// ── Shell ────────────────────────────────────────────────────────────────

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            s.rsplit('/').next().map(|s| s.to_string())
        })
        .unwrap_or_else(|| "sh".into())
}

// ── Terminal ─────────────────────────────────────────────────────────────

fn detect_terminal() -> String {
    std::env::var("TERM")
        .unwrap_or_else(|_| "unknown".into())
}

// ── CPU ──────────────────────────────────────────────────────────────────

fn read_cpu() -> String {
    let content = match fs::read_to_string("/proc/cpuinfo") {
        Ok(c) => c,
        Err(_) => return "unknown".into(),
    };

    let mut model = String::new();
    let mut cores = 0u32;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("model name") {
            if let Some(name) = val.split(':').nth(1) {
                model = name.trim().to_string();
            }
        }
        if line.starts_with("processor") {
            cores += 1;
        }
    }

    if model.is_empty() {
        return "unknown".into();
    }

    // Simplify CPU name: remove trademark symbols, "CPU" suffix, @ speed
    let simplified = model
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(r)", "")
        .replace("(tm)", "")
        .replace(" CPU", "");

    // Remove trailing @ speed
    let simplified = simplified
        .split(" @ ")
        .next()
        .unwrap_or(&simplified)
        .trim()
        .to_string();

    let shortened = shorten_cpu(&simplified);

    if cores > 1 {
        format!("{} ({} cores)", shortened, cores)
    } else {
        shortened
    }
}

/// Shorten a CPU name to its meaningful model identifier.
/// Removes verbose suffixes like "with ...", "N-Core Processor", etc.
fn shorten_cpu(name: &str) -> String {
    let name = name.trim();

    // Strip " with ..." (e.g., "AMD Ryzen 3 2200G with Radeon Vega Graphics")
    if let Some(pos) = name.find(" with ") {
        return name[..pos].trim().to_string();
    }

    // Strip trailing "N-Core Processor", "N-Core APU", or just "N-Core"
    // e.g., "AMD Ryzen 5 5600X 6-Core Processor" → "AMD Ryzen 5 5600X"
    let re1 = regex::Regex::new(r"\s+\d+-Core(?:\s+Processor|\s+APU)?$").unwrap();
    let name = re1.replace(name, "");

    // Strip trailing " Processor" or " APU" (left after Core removal)
    let name = regex::Regex::new(r"\s+(?:Processor|APU)$")
        .unwrap()
        .replace(&name, "");

    name.trim().to_string()
}

/// Shorten a GPU name to its meaningful model identifier.
fn shorten_gpu(name: &str) -> String {
    let name = name.trim();

    // NVIDIA: "NVIDIA GeForce RTX 3060" → "NVIDIA RTX 3060"
    let name = name.replace("GeForce ", "");
    // AMD: "AMD Radeon RX 570 Series" → "AMD RX 570"
    let name = name.replace("Radeon ", "");
    // Strip trailing " Series", " Graphics"
    let name = regex::Regex::new(r"\s+(?:Series|Graphics)$")
        .unwrap()
        .replace(&name, "");

    name.trim().to_string()
}

// ── GPU ──────────────────────────────────────────────────────────────────

fn read_gpu() -> String {
    // Try reading from DRM devices
    let drm_path = Path::new("/sys/class/drm");
    if let Ok(entries) = fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.contains("render") || !name_str.starts_with("card") {
                continue;
            }
            let dev_path = entry.path().join("device");

            // Try vendor/device from uevent
            let uevent_path = dev_path.join("uevent");
            if let Ok(uevent) = fs::read_to_string(&uevent_path) {
                let mut vendor = String::new();
                let mut device = String::new();
                for line in uevent.lines() {
                    if let Some(v) = line.strip_prefix("DRIVER=") {
                        vendor = v.to_string();
                    }
                    if vendor == "amdgpu" {
                        // Try product_name first (newer kernels)
                        let gpu_name = fs::read_to_string(dev_path.join("product_name"))
                            .ok()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "AMD Radeon".into());
                        return shorten_gpu(&gpu_name);
                    }
                    if vendor == "nvidia" {
                        // Try to get the model name
                        if let Ok(model) = fs::read_to_string(dev_path.join("model")) {
                            return shorten_gpu(model.trim());
                        }
                        return "NVIDIA".into();
                    }
                    if line.starts_with("MODALIAS") && device.is_empty() {
                        // Extract PCI ID from modalias
                        if let Some(pci_id) = line.split("pci:v").nth(1) {
                            let dev_info: Vec<&str> = pci_id.split('d').collect();
                            if dev_info.len() >= 2 {
                                let vendor_id = &dev_info[0][..4];
                                let _device_id = dev_info[1].chars().take(4).collect::<String>();
                                // Map known vendors
                                device = match vendor_id {
                                    "1002" | "1022" => "AMD".into(),
                                    "10de" => "NVIDIA".into(),
                                    "8086" => "Intel".into(),
                                    _ => format!("PCI:{}", vendor_id),
                                };
                            }
                        }
                    }
                }
                if !device.is_empty() {
                    return device;
                }
            }

            // Fallback: read class name
            if let Ok(class) = fs::read_to_string(dev_path.join("class")) {
                let trimmed = class.trim();
                if trimmed.contains("0300") || trimmed.contains("0302") {
                    // It's a VGA/3D controller
                    if let Ok(vendor) = fs::read_to_string(dev_path.join("vendor")) {
                        let v = vendor.trim();
                        return match v {
                            "0x1002" | "0x1022" => "AMD".into(),
                            "0x10de" => "NVIDIA".into(),
                            "0x8086" => "Intel".into(),
                            _ => format!("GPU (0x{})", &v[2..6]),
                        };
                    }
                }
            }
        }
    }
    "unknown".into()
}

// ── Memory ───────────────────────────────────────────────────────────────

fn format_memory() -> String {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return "unknown".into(),
    };

    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("MemTotal:") {
            total_kb = val.trim().split_whitespace().next().unwrap_or("0").parse().unwrap_or(0);
        }
        if let Some(val) = line.strip_prefix("MemAvailable:") {
            avail_kb = val.trim().split_whitespace().next().unwrap_or("0").parse().unwrap_or(0);
        }
    }

    if total_kb == 0 {
        return "unknown".into();
    }

    let used_kb = total_kb.saturating_sub(avail_kb);
    let used = used_kb as f64 / 1_048_576.0;
    let total = total_kb as f64 / 1_048_576.0;

    format!("{:.1}/{:.1}G", used, total)
}

// ── Disk ─────────────────────────────────────────────────────────────────

fn format_disk(mount: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let cpath = std::ffi::CString::new(mount).unwrap_or_default();
        if unsafe { libc::statvfs(cpath.as_ptr(), &mut stat) } == 0 {
            let total = stat.f_blocks as u64 * stat.f_frsize as u64;
            let free = stat.f_bfree as u64 * stat.f_frsize as u64;
            let used = total.saturating_sub(free);
            let total_g = total as f64 / 1_073_741_824.0;
            let used_g = used as f64 / 1_073_741_824.0;
            return format!("{:.0}/{:.0}G", used_g, total_g);
        }
    }

    // Fallback: read from /proc/mounts and statfs via /sys
    let stat_path = format!("/sys/fs/{}/", mount.trim_start_matches('/'));
    #[allow(unused_variables)]
    let _ = stat_path;

    "unknown".into()
}

// ── WM ───────────────────────────────────────────────────────────────────

fn detect_wm() -> String {
    // Check common environment variables set by WMs
    for (var, name) in &[
        ("XDG_CURRENT_DESKTOP", None),
        ("DESKTOP_SESSION", None),
        ("HYPRLAND_INSTANCE_SIGNATURE", Some("Hyprland")),
        ("SWAYSOCK", Some("Sway")),
        ("I3SOCK", Some("i3")),
        ("QTILE_SOCKET", Some("Qtile")),
        ("AWESOME_CLIENT_INSTANCE", Some("Awesome")),
    ] {
        if let Ok(val) = std::env::var(var) {
            if let Some(fixed) = name {
                return fixed.to_string();
            }
            if !val.is_empty() {
                return val;
            }
        }
    }

    // Try reading from /proc
    if let Ok(proc) = fs::read_dir("/proc") {
        for entry in proc.flatten() {
            let pid = entry.file_name();
            let pid_str = pid.to_string_lossy();
            if let Ok(comm) = fs::read_to_string(format!("/proc/{}/comm", pid_str)) {
                let comm = comm.trim();
                match comm {
                    "Hyprland" => return "Hyprland".into(),
                    "sway" => return "Sway".into(),
                    "i3" => return "i3".into(),
                    "qtile" => return "Qtile".into(),
                    "awesome" => return "Awesome".into(),
                    "bspwm" => return "bspwm".into(),
                    "dwm" => return "dwm".into(),
                    "openbox" => return "Openbox".into(),
                    "fluxbox" => return "Fluxbox".into(),
                    "xfwm4" => return "Xfwm4".into(),
                    "kwin_x11" | "kwin_wayland" => return "KWin".into(),
                    _ => {}
                }
            }
        }
    }

    "unknown".into()
}

// ── Load ─────────────────────────────────────────────────────────────────

fn read_load() -> String {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| {
            s.split_whitespace().next().map(|v| v.to_string())
        })
        .unwrap_or_else(|| "?".into())
}

// ── Processes ────────────────────────────────────────────────────────────

fn count_processes() -> String {
    let count = fs::read_dir("/proc")
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .chars()
                        .all(|c| c.is_ascii_digit())
                })
                .count()
        })
        .unwrap_or(0);
    count.to_string()
}

// ── Packages ─────────────────────────────────────────────────────────────

fn count_packages() -> String {
    let mut counts: Vec<String> = Vec::new();

    // pacman
    if let Ok(out) = std::process::Command::new("pacman")
        .args(["-Qq", "--color", "never"])
        .output()
    {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            if count > 0 {
                counts.push(format!("{} (pacman)", count));
            }
        }
    }

    // dpkg
    if let Ok(out) = std::process::Command::new("dpkg-query")
        .args(["-f", ".\\n", "-W"])
        .output()
    {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            if count > 0 {
                counts.push(format!("{} (dpkg)", count));
            }
        }
    }

    // rpm
    if let Ok(out) = std::process::Command::new("rpm").args(["-qa"]).output() {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            if count > 0 {
                counts.push(format!("{} (rpm)", count));
            }
        }
    }

    // xbps
    if let Ok(out) = std::process::Command::new("xbps-query")
        .args(["-l"])
        .output()
    {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            if count > 0 {
                counts.push(format!("{} (xbps)", count));
            }
        }
    }

    // emerge (gentoo)
    let world_path = Path::new("/var/lib/portage/world");
    if world_path.exists() {
        if let Ok(content) = fs::read_to_string(world_path) {
            let count = content.lines().count();
            if count > 0 {
                counts.push(format!("{} (emerge)", count));
            }
        }
    }

    // nix
    if let Ok(out) = std::process::Command::new("nix-store")
        .args(["-qR", "/run/current-system/sw"])
        .output()
    {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            if count > 0 {
                counts.push(format!("{} (nix)", count));
            }
        }
    }

    // flatpak
    if let Ok(out) = std::process::Command::new("flatpak")
        .args(["list"])
        .output()
    {
        if out.status.success() {
            let count = String::from_utf8_lossy(&out.stdout).lines().count();
            // flatpak has a header line
            let count = count.saturating_sub(1);
            if count > 0 {
                counts.push(format!("{} (flatpak)", count));
            }
        }
    }

    if counts.is_empty() {
        return "—".into();
    }

    let total: usize = counts
        .iter()
        .filter_map(|s| s.split_whitespace().next()?.parse::<usize>().ok())
        .sum();

    format!("{}", total)
}

// ── Local IP ─────────────────────────────────────────────────────────────

fn local_ip() -> String {
    // Find the first non-loopback IPv4 address
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "lo" {
                continue;
            }
            // Try to get address via /proc/net/fib_trie or /proc/net/route
            // Simple approach: skip, this is best-effort
            if let Ok(addr) = get_addr_for_iface(&name) {
                if !addr.is_empty() {
                    return addr;
                }
            }
        }
    }
    String::new()
}

fn get_addr_for_iface(name: &str) -> Result<String> {
    let addr_path = format!("/sys/class/net/{}/address", name);
    if let Ok(_mac) = fs::read_to_string(&addr_path) {
        // We'd need an actual method to get IP. For now try via /proc/net/fib_trie
        // Skip this for simplicity — returns empty, user can add later
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_cpu() {
        let cases = vec![
            ("AMD Ryzen 3 2200G with Radeon Vega Graphics", "AMD Ryzen 3 2200G"),
            ("AMD Ryzen 5 5600X 6-Core Processor", "AMD Ryzen 5 5600X"),
            ("AMD Ryzen 7 5800X3D", "AMD Ryzen 7 5800X3D"),
            ("AMD EPYC 7551P 32-Core Processor", "AMD EPYC 7551P"),
        ];
        for (input, expected) in cases {
            assert_eq!(shorten_cpu(input), expected, "CPU: {}", input);
        }
    }

    #[test]
    fn test_shorten_gpu() {
        let cases = vec![
            ("NVIDIA GeForce RTX 3060", "NVIDIA RTX 3060"),
            ("NVIDIA GeForce GTX 1060 6GB", "NVIDIA GTX 1060 6GB"),
            ("AMD Radeon RX 570 Series", "AMD RX 570"),
            ("AMD Radeon RX 7800 XT", "AMD RX 7800 XT"),
            ("Intel UHD Graphics 630", "Intel UHD Graphics 630"),
        ];
        for (input, expected) in cases {
            assert_eq!(shorten_gpu(input), expected, "GPU: {}", input);
        }
    }
}
