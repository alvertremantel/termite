use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

pub const DEFAULT_POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

#[derive(Clone, Debug, PartialEq)]
pub struct BatterySummary {
    pub name: String,
    pub path: PathBuf,
    pub status: Option<String>,
    pub capacity_percent: Option<u8>,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_full_design_wh: Option<f64>,
    pub power_now_w: Option<f64>,
    pub voltage_now_v: Option<f64>,
    pub cycle_count: Option<u64>,
    pub health_percent: Option<f64>,
    pub hours_remaining: Option<f64>,
    pub hours_to_full: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryFleetSummary {
    pub batteries: Vec<BatterySummary>,
    pub aggregate_status: Option<String>,
    pub total_capacity_percent: Option<u8>,
    pub total_energy_now_wh: Option<f64>,
    pub total_energy_full_wh: Option<f64>,
    pub total_power_now_w: Option<f64>,
    pub hours_remaining: Option<f64>,
    pub hours_to_full: Option<f64>,
}

pub fn read_battery_fleet(path: impl AsRef<Path>) -> Result<BatteryFleetSummary> {
    let path = path.as_ref();
    let entries =
        fs::read_dir(path).map_err(|error| Error::io(format!("scan {}", path.display()), error))?;

    let mut batteries = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::io("read power supply entry", error))?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };

        if !looks_like_battery(&entry_path, &name)? {
            continue;
        }

        batteries.push(read_battery(&entry_path, name)?);
    }

    batteries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(summarize_fleet(batteries))
}

fn looks_like_battery(path: &Path, name: &str) -> Result<bool> {
    if name.starts_with("BAT") {
        return Ok(true);
    }

    Ok(read_trimmed(path.join("type"))?.is_some_and(|kind| kind.eq_ignore_ascii_case("battery")))
}

fn read_battery(path: &Path, name: String) -> Result<BatterySummary> {
    let status = read_trimmed(path.join("status"))?;
    let voltage_now_v = read_scaled_f64(path.join("voltage_now"), 1_000_000.0)?;
    let energy_now_wh = read_energy_now_wh(path, voltage_now_v)?;
    let energy_full_wh = read_energy_full_wh(path, voltage_now_v)?;
    let energy_full_design_wh = read_energy_full_design_wh(path, voltage_now_v)?;
    let power_now_w = read_power_now_w(path, voltage_now_v)?;
    let cycle_count = read_u64(path.join("cycle_count"))?;

    let capacity_percent = read_u64(path.join("capacity"))?
        .and_then(|value| u8::try_from(value.min(100)).ok())
        .or_else(|| percent_from_energy(energy_now_wh, energy_full_wh));

    let health_percent = match (energy_full_wh, energy_full_design_wh) {
        (Some(full), Some(design)) if design > 0.0 => Some((full / design) * 100.0),
        _ => None,
    };

    let normalized_status = status.as_deref().map(normalize_status);
    let hours_remaining = match (normalized_status, energy_now_wh, power_now_w) {
        (Some("discharging"), Some(energy), Some(power)) if power > 0.0 => Some(energy / power),
        _ => None,
    };
    let hours_to_full = match (
        normalized_status,
        energy_now_wh,
        energy_full_wh,
        power_now_w,
    ) {
        (Some("charging"), Some(now), Some(full), Some(power)) if full > now && power > 0.0 => {
            Some((full - now) / power)
        }
        _ => None,
    };

    Ok(BatterySummary {
        name,
        path: path.to_path_buf(),
        status,
        capacity_percent,
        energy_now_wh,
        energy_full_wh,
        energy_full_design_wh,
        power_now_w,
        voltage_now_v,
        cycle_count,
        health_percent,
        hours_remaining,
        hours_to_full,
    })
}

fn summarize_fleet(batteries: Vec<BatterySummary>) -> BatteryFleetSummary {
    let total_energy_now_wh = sum_option_f64(batteries.iter().map(|battery| battery.energy_now_wh));
    let total_energy_full_wh =
        sum_option_f64(batteries.iter().map(|battery| battery.energy_full_wh));
    let total_power_now_w = sum_option_f64(batteries.iter().map(|battery| battery.power_now_w));

    let total_capacity_percent = total_energy_now_wh
        .zip(total_energy_full_wh)
        .and_then(|(now, full)| {
            if full > 0.0 {
                Some(((now / full) * 100.0).round())
            } else {
                None
            }
        })
        .map(|value| value.clamp(0.0, 100.0) as u8)
        .or_else(|| {
            average_u8(
                batteries
                    .iter()
                    .filter_map(|battery| battery.capacity_percent),
            )
        });

    let aggregate_status = summarize_status(&batteries);
    let hours_remaining = match (
        aggregate_status.as_deref(),
        total_energy_now_wh,
        total_power_now_w,
    ) {
        (Some("discharging"), Some(energy), Some(power)) if power > 0.0 => Some(energy / power),
        _ => None,
    };
    let hours_to_full = match (
        aggregate_status.as_deref(),
        total_energy_now_wh,
        total_energy_full_wh,
        total_power_now_w,
    ) {
        (Some("charging"), Some(now), Some(full), Some(power)) if full > now && power > 0.0 => {
            Some((full - now) / power)
        }
        _ => None,
    };

    BatteryFleetSummary {
        batteries,
        aggregate_status,
        total_capacity_percent,
        total_energy_now_wh,
        total_energy_full_wh,
        total_power_now_w,
        hours_remaining,
        hours_to_full,
    }
}

fn summarize_status(batteries: &[BatterySummary]) -> Option<String> {
    let mut statuses = batteries
        .iter()
        .filter_map(|battery| battery.status.as_deref())
        .map(normalize_status)
        .collect::<Vec<_>>();

    statuses.sort_unstable();
    statuses.dedup();

    match statuses.as_slice() {
        [] => None,
        [status] => Some((*status).to_string()),
        slice if slice.contains(&"discharging") => Some("discharging".into()),
        slice if slice.contains(&"charging") => Some("charging".into()),
        _ => Some("mixed".into()),
    }
}

fn normalize_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "charging" => "charging",
        "discharging" => "discharging",
        "full" => "full",
        "not charging" => "not charging",
        _ => "unknown",
    }
}

fn read_energy_now_wh(path: &Path, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    read_scaled_f64(path.join("energy_now"), 1_000_000.0).and_then(|energy| match energy {
        Some(value) => Ok(Some(value)),
        None => read_charge_wh(path.join("charge_now"), voltage_now_v),
    })
}

fn read_energy_full_wh(path: &Path, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    read_scaled_f64(path.join("energy_full"), 1_000_000.0).and_then(|energy| match energy {
        Some(value) => Ok(Some(value)),
        None => read_charge_wh(path.join("charge_full"), voltage_now_v),
    })
}

fn read_energy_full_design_wh(path: &Path, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    read_scaled_f64(path.join("energy_full_design"), 1_000_000.0).and_then(|energy| match energy {
        Some(value) => Ok(Some(value)),
        None => read_charge_wh(path.join("charge_full_design"), voltage_now_v),
    })
}

fn read_power_now_w(path: &Path, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    read_scaled_f64(path.join("power_now"), 1_000_000.0).and_then(|power| match power {
        Some(value) => Ok(Some(value)),
        None => read_current_w(path.join("current_now"), voltage_now_v),
    })
}

fn read_charge_wh(path: PathBuf, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    let Some(charge_ah) = read_scaled_f64(path, 1_000_000.0)? else {
        return Ok(None);
    };
    Ok(voltage_now_v.map(|voltage| charge_ah * voltage))
}

fn read_current_w(path: PathBuf, voltage_now_v: Option<f64>) -> Result<Option<f64>> {
    let Some(current_a) = read_scaled_f64(path, 1_000_000.0)? else {
        return Ok(None);
    };
    Ok(voltage_now_v.map(|voltage| current_a * voltage))
}

fn percent_from_energy(now: Option<f64>, full: Option<f64>) -> Option<u8> {
    match (now, full) {
        (Some(now), Some(full)) if full > 0.0 => {
            Some(((now / full) * 100.0).round().clamp(0.0, 100.0) as u8)
        }
        _ => None,
    }
}

fn sum_option_f64(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let mut saw_value = false;
    let mut total = 0.0;
    for value in values.flatten() {
        saw_value = true;
        total += value;
    }
    saw_value.then_some(total)
}

fn average_u8(values: impl Iterator<Item = u8>) -> Option<u8> {
    let mut count = 0_u64;
    let mut total = 0_u64;
    for value in values {
        count += 1;
        total += u64::from(value);
    }
    total.checked_div(count).map(|average| average as u8)
}

fn read_trimmed(path: PathBuf) -> Result<Option<String>> {
    match fs::read_to_string(&path) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io(format!("read {}", path.display()), error)),
    }
}

fn read_u64(path: PathBuf) -> Result<Option<u64>> {
    let Some(value) = read_trimmed(path.clone())? else {
        return Ok(None);
    };
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|error| Error::ParseFailure {
            context: path.display().to_string(),
            details: error.to_string(),
        })
}

fn read_scaled_f64(path: PathBuf, divisor: f64) -> Result<Option<f64>> {
    let Some(value) = read_trimmed(path.clone())? else {
        return Ok(None);
    };
    value
        .parse::<f64>()
        .map(|raw| Some(raw / divisor))
        .map_err(|error| Error::ParseFailure {
            context: path.display().to_string(),
            details: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_supply_file(dir: &Path, name: &str, value: impl AsRef<str>) {
        fs::write(dir.join(name), value.as_ref()).unwrap();
    }

    #[test]
    fn reads_energy_based_battery_and_computes_hours_remaining() {
        let root = tempdir().unwrap();
        let bat0 = root.path().join("BAT0");
        fs::create_dir(&bat0).unwrap();
        write_supply_file(&bat0, "type", "Battery\n");
        write_supply_file(&bat0, "status", "Discharging\n");
        write_supply_file(&bat0, "capacity", "75\n");
        write_supply_file(&bat0, "energy_now", "45000000\n");
        write_supply_file(&bat0, "energy_full", "60000000\n");
        write_supply_file(&bat0, "energy_full_design", "70000000\n");
        write_supply_file(&bat0, "power_now", "15000000\n");
        write_supply_file(&bat0, "voltage_now", "12000000\n");
        write_supply_file(&bat0, "cycle_count", "212\n");

        let fleet = read_battery_fleet(root.path()).unwrap();
        let battery = &fleet.batteries[0];
        assert_eq!(fleet.total_capacity_percent, Some(75));
        assert_eq!(fleet.aggregate_status.as_deref(), Some("discharging"));
        assert_eq!(battery.energy_now_wh, Some(45.0));
        assert_eq!(battery.power_now_w, Some(15.0));
        assert_eq!(battery.hours_remaining, Some(3.0));
        assert_eq!(
            battery.health_percent.map(|value| value.round() as u64),
            Some(86)
        );
    }

    #[test]
    fn falls_back_to_charge_and_current_metrics() {
        let root = tempdir().unwrap();
        let bat0 = root.path().join("battery-main");
        fs::create_dir(&bat0).unwrap();
        write_supply_file(&bat0, "type", "Battery\n");
        write_supply_file(&bat0, "status", "Charging\n");
        write_supply_file(&bat0, "charge_now", "4000000\n");
        write_supply_file(&bat0, "charge_full", "5000000\n");
        write_supply_file(&bat0, "charge_full_design", "5500000\n");
        write_supply_file(&bat0, "current_now", "2000000\n");
        write_supply_file(&bat0, "voltage_now", "10000000\n");

        let fleet = read_battery_fleet(root.path()).unwrap();
        let battery = &fleet.batteries[0];
        assert_eq!(battery.capacity_percent, Some(80));
        assert_eq!(battery.energy_now_wh, Some(40.0));
        assert_eq!(battery.energy_full_wh, Some(50.0));
        assert_eq!(battery.power_now_w, Some(20.0));
        assert_eq!(battery.hours_to_full, Some(0.5));
        assert_eq!(fleet.hours_to_full, Some(0.5));
    }

    #[test]
    fn ignores_non_battery_supplies() {
        let root = tempdir().unwrap();
        let ac = root.path().join("AC");
        fs::create_dir(&ac).unwrap();
        write_supply_file(&ac, "type", "Mains\n");

        let fleet = read_battery_fleet(root.path()).unwrap();
        assert!(fleet.batteries.is_empty());
        assert_eq!(fleet.total_capacity_percent, None);
    }
}
