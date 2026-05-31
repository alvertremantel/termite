use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

pub const DEFAULT_PROC_DIR: &str = "/proc";
pub const DEFAULT_SYS_DIR: &str = "/sys";

#[derive(Clone, Debug, PartialEq)]
pub struct UsageSummary {
    pub cpu_percent: Option<f64>,
    pub ram: Option<RamUsage>,
    pub gpus: Vec<GpuUsage>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RamUsage {
    pub total_bytes: u64,
    pub available_bytes: Option<u64>,
    pub used_bytes: u64,
    pub used_percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuUsage {
    pub name: String,
    pub utilization_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }

    fn idle_total(&self) -> u64 {
        self.idle + self.iowait
    }

    fn checked_deltas(&self, previous: &Self) -> Option<Self> {
        Some(Self {
            user: self.user.checked_sub(previous.user)?,
            nice: self.nice.checked_sub(previous.nice)?,
            system: self.system.checked_sub(previous.system)?,
            idle: self.idle.checked_sub(previous.idle)?,
            iowait: self.iowait.checked_sub(previous.iowait)?,
            irq: self.irq.checked_sub(previous.irq)?,
            softirq: self.softirq.checked_sub(previous.softirq)?,
            steal: self.steal.checked_sub(previous.steal)?,
            guest: self.guest.checked_sub(previous.guest)?,
            guest_nice: self.guest_nice.checked_sub(previous.guest_nice)?,
        })
    }
}

pub fn read_usage_summary(
    proc_root: impl AsRef<Path>,
    sys_root: impl AsRef<Path>,
    previous_cpu: Option<&CpuTimes>,
) -> Result<UsageSummary> {
    let current_cpu = read_cpu_times(proc_root.as_ref())?;
    read_usage_summary_with_cpu_times(proc_root, sys_root, previous_cpu, &current_cpu)
}

pub fn read_usage_summary_with_cpu_times(
    proc_root: impl AsRef<Path>,
    sys_root: impl AsRef<Path>,
    previous_cpu: Option<&CpuTimes>,
    current_cpu: &CpuTimes,
) -> Result<UsageSummary> {
    let cpu_percent =
        previous_cpu.and_then(|previous| calculate_cpu_percent(previous, current_cpu));
    let ram = Some(read_meminfo(proc_root)?);
    let gpus = read_gpu_summaries(sys_root)?;

    Ok(UsageSummary {
        cpu_percent,
        ram,
        gpus,
    })
}

pub fn read_meminfo(proc_root: impl AsRef<Path>) -> Result<RamUsage> {
    let path = proc_root.as_ref().join("meminfo");
    let content = fs::read_to_string(&path)
        .map_err(|error| Error::io(format!("read {}", path.display()), error))?;
    parse_meminfo(&content, &path)
}

pub fn read_cpu_times(proc_root: impl AsRef<Path>) -> Result<CpuTimes> {
    let path = proc_root.as_ref().join("stat");
    let content = fs::read_to_string(&path)
        .map_err(|error| Error::io(format!("read {}", path.display()), error))?;
    parse_cpu_times(&content, &path)
}

pub fn calculate_cpu_percent(previous: &CpuTimes, current: &CpuTimes) -> Option<f64> {
    let deltas = current.checked_deltas(previous)?;
    let total_delta = deltas.total();
    if total_delta == 0 {
        return None;
    }

    let idle_delta = deltas.idle_total();
    let busy_delta = total_delta.saturating_sub(idle_delta);
    Some(((busy_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0))
}

pub fn read_gpu_summaries(sys_root: impl AsRef<Path>) -> Result<Vec<GpuUsage>> {
    let drm_path = sys_root.as_ref().join("class").join("drm");
    let entries = match fs::read_dir(&drm_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::io(format!("scan {}", drm_path.display()), error)),
    };

    let mut gpus = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("read drm entry", error))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_drm_card_dir(&name) {
            continue;
        }

        gpus.push(read_gpu_summary(path, name)?);
    }

    gpus.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(gpus)
}

fn parse_meminfo(content: &str, path: &Path) -> Result<RamUsage> {
    let mut total = None;
    let mut available = None;
    let mut free = None;
    let mut buffers = None;
    let mut cached = None;
    let mut sreclaimable = None;
    let mut shmem = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };

        let parsed = match key {
            "MemTotal" | "MemAvailable" | "MemFree" | "Buffers" | "Cached" | "SReclaimable"
            | "Shmem" => Some(parse_meminfo_kib(value, path, key)?),
            _ => None,
        };

        match key {
            "MemTotal" => total = parsed,
            "MemAvailable" => available = parsed,
            "MemFree" => free = parsed,
            "Buffers" => buffers = parsed,
            "Cached" => cached = parsed,
            "SReclaimable" => sreclaimable = parsed,
            "Shmem" => shmem = parsed,
            _ => {}
        }
    }

    let total_bytes = total.ok_or_else(|| Error::ParseFailure {
        context: path.display().to_string(),
        details: "missing MemTotal".into(),
    })?;
    let available_bytes = available.or_else(|| {
        Some(
            free?
                .saturating_add(buffers.unwrap_or(0))
                .saturating_add(cached.unwrap_or(0))
                .saturating_add(sreclaimable.unwrap_or(0))
                .saturating_sub(shmem.unwrap_or(0)),
        )
    });
    let used_bytes = available_bytes
        .map(|available| total_bytes.saturating_sub(available))
        .unwrap_or(total_bytes);
    let used_percent = if total_bytes > 0 {
        ((used_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    Ok(RamUsage {
        total_bytes,
        available_bytes,
        used_bytes,
        used_percent,
    })
}

fn parse_meminfo_kib(value: &str, path: &Path, key: &str) -> Result<u64> {
    let raw = value
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::ParseFailure {
            context: format!("{} {key}", path.display()),
            details: "missing numeric value".into(),
        })?;
    raw.parse::<u64>()
        .map(|kib| kib.saturating_mul(1024))
        .map_err(|error| Error::ParseFailure {
            context: format!("{} {key}", path.display()),
            details: error.to_string(),
        })
}

fn parse_cpu_times(content: &str, path: &Path) -> Result<CpuTimes> {
    let line = content
        .lines()
        .find(|line| line.strip_prefix("cpu ").is_some())
        .ok_or_else(|| Error::ParseFailure {
            context: path.display().to_string(),
            details: "missing aggregate cpu line".into(),
        })?;

    let mut values = line.split_whitespace().skip(1);
    Ok(CpuTimes {
        user: parse_cpu_field(values.next(), path, "user")?,
        nice: parse_cpu_field(values.next(), path, "nice")?,
        system: parse_cpu_field(values.next(), path, "system")?,
        idle: parse_cpu_field(values.next(), path, "idle")?,
        iowait: parse_optional_cpu_field(values.next(), path, "iowait")?,
        irq: parse_optional_cpu_field(values.next(), path, "irq")?,
        softirq: parse_optional_cpu_field(values.next(), path, "softirq")?,
        steal: parse_optional_cpu_field(values.next(), path, "steal")?,
        guest: parse_optional_cpu_field(values.next(), path, "guest")?,
        guest_nice: parse_optional_cpu_field(values.next(), path, "guest_nice")?,
    })
}

fn parse_cpu_field(value: Option<&str>, path: &Path, field: &str) -> Result<u64> {
    let value = value.ok_or_else(|| Error::ParseFailure {
        context: format!("{} cpu {field}", path.display()),
        details: "missing numeric value".into(),
    })?;
    value.parse::<u64>().map_err(|error| Error::ParseFailure {
        context: format!("{} cpu {field}", path.display()),
        details: error.to_string(),
    })
}

fn parse_optional_cpu_field(value: Option<&str>, path: &Path, field: &str) -> Result<u64> {
    match value {
        Some(value) => value.parse::<u64>().map_err(|error| Error::ParseFailure {
            context: format!("{} cpu {field}", path.display()),
            details: error.to_string(),
        }),
        None => Ok(0),
    }
}

fn read_gpu_summary(path: PathBuf, directory_name: String) -> Result<GpuUsage> {
    let device_path = path.join("device");
    let name = infer_gpu_name(&device_path).unwrap_or(directory_name);
    let utilization_percent = read_optional_u64_quiet(device_path.join("gpu_busy_percent"))
        .map(|value| (value as f64).clamp(0.0, 100.0));
    let memory_used_bytes = read_optional_u64_quiet(device_path.join("mem_info_vram_used"));
    let memory_total_bytes = read_optional_u64_quiet(device_path.join("mem_info_vram_total"));

    Ok(GpuUsage {
        name,
        utilization_percent,
        memory_used_bytes,
        memory_total_bytes,
    })
}

fn infer_gpu_name(device_path: &Path) -> Option<String> {
    let vendor = read_trimmed_quiet(device_path.join("vendor"))?;
    let device = read_trimmed_quiet(device_path.join("device"))?;
    Some(format!(
        "{}:{}",
        vendor.trim_start_matches("0x"),
        device.trim_start_matches("0x")
    ))
}

fn is_drm_card_dir(name: &str) -> bool {
    name.strip_prefix("card").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

fn read_optional_u64_quiet(path: PathBuf) -> Option<u64> {
    read_trimmed_quiet(path)?.parse::<u64>().ok()
}

fn read_trimmed_quiet(path: PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_file(path: impl AsRef<Path>, value: impl AsRef<str>) {
        fs::write(path, value.as_ref()).unwrap();
    }

    #[test]
    fn reads_meminfo_with_memavailable() {
        let root = tempdir().unwrap();
        write_file(
            root.path().join("meminfo"),
            "MemTotal:       1000 kB\nMemAvailable:    250 kB\n",
        );

        let usage = read_meminfo(root.path()).unwrap();

        assert_eq!(usage.total_bytes, 1_024_000);
        assert_eq!(usage.available_bytes, Some(256_000));
        assert_eq!(usage.used_bytes, 768_000);
        assert_eq!(usage.used_percent, 75.0);
    }

    #[test]
    fn reads_meminfo_with_fallback_available_estimate() {
        let root = tempdir().unwrap();
        write_file(
            root.path().join("meminfo"),
            "\
MemTotal:       2000 kB
MemFree:         200 kB
Buffers:         100 kB
Cached:          300 kB
SReclaimable:     50 kB
Shmem:            25 kB
",
        );

        let usage = read_meminfo(root.path()).unwrap();

        assert_eq!(usage.available_bytes, Some(625 * 1024));
        assert_eq!(usage.used_bytes, 1375 * 1024);
        assert_eq!(usage.used_percent, 68.75);
    }

    #[test]
    fn cpu_delta_calculates_busy_percent() {
        let previous = CpuTimes {
            user: 100,
            nice: 0,
            system: 50,
            idle: 850,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
            guest: 0,
            guest_nice: 0,
        };
        let current = CpuTimes {
            user: 170,
            nice: 0,
            system: 60,
            idle: 870,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
            guest: 0,
            guest_nice: 0,
        };

        assert_eq!(calculate_cpu_percent(&previous, &current), Some(80.0));
        assert_eq!(calculate_cpu_percent(&current, &previous), None);
        assert_eq!(calculate_cpu_percent(&previous, &previous), None);

        let regressed_idle = CpuTimes {
            user: 300,
            idle: 800,
            ..previous.clone()
        };
        assert_eq!(calculate_cpu_percent(&previous, &regressed_idle), None);
    }

    #[test]
    fn cpu_delta_does_not_double_count_guest_time() {
        let previous = CpuTimes {
            user: 100,
            nice: 0,
            system: 0,
            idle: 900,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
            guest: 20,
            guest_nice: 0,
        };
        let current = CpuTimes {
            user: 200,
            nice: 0,
            system: 0,
            idle: 1000,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
            guest: 70,
            guest_nice: 0,
        };

        assert_eq!(calculate_cpu_percent(&previous, &current), Some(50.0));
    }

    #[test]
    fn malformed_meminfo_number_has_context() {
        let root = tempdir().unwrap();
        let path = root.path().join("meminfo");
        write_file(&path, "MemTotal: not-a-number kB\n");

        let error = read_meminfo(root.path()).unwrap_err();

        assert!(error.to_string().contains(path.to_str().unwrap()));
        assert!(error.to_string().contains("MemTotal"));
    }

    #[test]
    fn missing_gpu_path_is_empty() {
        let root = tempdir().unwrap();

        let gpus = read_gpu_summaries(root.path()).unwrap();

        assert!(gpus.is_empty());
    }

    #[test]
    fn reads_amd_style_gpu_fixture() {
        let root = tempdir().unwrap();
        let drm = root.path().join("class").join("drm");
        let card = drm.join("card0");
        let connector = drm.join("card0-DP-1");
        let device = card.join("device");
        fs::create_dir_all(&device).unwrap();
        fs::create_dir_all(connector).unwrap();
        write_file(device.join("vendor"), "0x1002\n");
        write_file(device.join("device"), "0x164e\n");
        write_file(device.join("gpu_busy_percent"), "37\n");
        write_file(device.join("mem_info_vram_used"), "268435456\n");
        write_file(device.join("mem_info_vram_total"), "1073741824\n");

        let gpus = read_gpu_summaries(root.path()).unwrap();

        assert_eq!(
            gpus,
            vec![GpuUsage {
                name: "1002:164e".into(),
                utilization_percent: Some(37.0),
                memory_used_bytes: Some(268_435_456),
                memory_total_bytes: Some(1_073_741_824),
            }]
        );
    }
}
