use jtop_core::{ActionRisk, BatteryFleetSummary, BatterySummary, CommandRunner, TlpConfigState};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Sparkline, Wrap},
};

use crate::{app::App, theme};

pub fn render<R: CommandRunner + Clone + Send + Sync + 'static>(frame: &mut Frame, app: &App<R>) {
    let size = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(16),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(size);

    render_title(frame, vertical[0], app);
    render_body(frame, vertical[1], app);
    render_logs(frame, vertical[2], app);
    render_footer(frame, vertical[3]);

    if let Some(area) = confirmation_area(size, app.pending_spec().is_some() || app.show_help) {
        if app.show_help {
            render_help(frame, area);
        } else if app.pending_spec().is_some() {
            render_confirmation(frame, area, app);
        }
    }
}

fn render_title<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let badge = if app.privilege.effective_root {
        Line::from("root").style(theme::badge_root())
    } else if app.privilege.sudo_available {
        Line::from("sudo ready").style(theme::badge_sudo())
    } else if app.discovery.sudo.present {
        Line::from("sudo on run").style(theme::badge_sudo())
    } else {
        Line::from("read-only").style(theme::badge_read_only())
    };

    let mut lines = vec![
        Line::from("jtop · Linux power cockpit").style(theme::title()),
        badge,
    ];
    if let Some(battery) = &app.battery {
        lines.push(Line::from(format!(
            "battery: {} · draw: {} · {} · auto-refresh every 3m",
            battery
                .total_capacity_percent
                .map(|value| format!("{value}%"))
                .unwrap_or_else(|| "n/a".into()),
            battery
                .total_power_now_w
                .map(|value| format!("{value:.1}W"))
                .unwrap_or_else(|| "n/a".into()),
            battery_life_label(battery),
        )));
    }
    if let Some(message) = app.busy_message.as_deref() {
        lines.push(Line::from(format!("busy: {message}")));
    } else {
        lines.push(Line::from(format!(
            "last refresh: {}",
            app.last_refresh_completed_at
                .map(|instant| format_duration(instant.elapsed().as_secs()))
                .unwrap_or_else(|| "starting up".into())
        )));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(paragraph, area);
}

fn render_body<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let chunks = if area.width >= 100 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
    };

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(10)])
        .split(chunks[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(12),
        ])
        .split(chunks[1]);

    render_battery_deck(frame, left[0], app);
    render_live_readouts(frame, left[1], app);
    render_tlp_status(frame, right[0], app);
    render_powertop(frame, right[1], app);
    render_system_panels(frame, right[2], app);
}

fn render_battery_deck<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let lines = if let Some(fleet) = &app.battery {
        let mut lines = vec![Line::from(vec![
            Span::styled(
                fleet
                    .total_capacity_percent
                    .map(|value| format!("{value}%"))
                    .unwrap_or_else(|| "--%".into()),
                theme::title(),
            ),
            Span::raw("  "),
            Span::raw(battery_life_label(fleet)),
            Span::raw("  "),
            Span::raw(battery_status_label(fleet.aggregate_status.as_deref())),
        ])];

        if fleet.batteries.is_empty() {
            lines.push(Line::from("No battery packs detected."));
        } else {
            for battery in &fleet.batteries {
                lines.push(render_battery_line(battery));
            }
        }

        if let (Some(now), Some(full)) = (fleet.total_energy_now_wh, fleet.total_energy_full_wh) {
            lines.push(Line::from(format!("energy: {now:.1}Wh / {full:.1}Wh")));
        }
        lines
    } else {
        vec![Line::from(
            "No battery telemetry yet. jtop refreshes live stats every 3 minutes.",
        )]
    };

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("Battery deck"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_live_readouts<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(area);

    let lines = if let Some(fleet) = &app.battery {
        vec![
            Line::from(format!(
                "usage rate: {}",
                fleet
                    .total_power_now_w
                    .map(|value| format!("{value:.1}W"))
                    .unwrap_or_else(|| "n/a".into())
            )),
            Line::from(format!(
                "life remaining: {}",
                fleet
                    .hours_remaining
                    .map(format_hours)
                    .unwrap_or_else(|| fleet
                        .hours_to_full
                        .map(|hours| format!("{} to full", format_hours(hours)))
                        .unwrap_or_else(|| "n/a".into()))
            )),
            Line::from(format!(
                "battery health: {}",
                average_health(fleet)
                    .map(|value| format!("{value:.0}%"))
                    .unwrap_or_else(|| "n/a".into())
            )),
            Line::from(format!(
                "packs: {} · cycles: {}",
                fleet.batteries.len(),
                cycle_summary(fleet)
            )),
            Line::from(format!(
                "voltage: {}",
                fleet
                    .batteries
                    .iter()
                    .find_map(|battery| battery.voltage_now_v)
                    .map(|value| format!("{value:.2}V"))
                    .unwrap_or_else(|| "n/a".into())
            )),
        ]
    } else {
        vec![Line::from("Waiting for battery telemetry.")]
    };

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Live telemetry"),
            )
            .wrap(Wrap { trim: true }),
        chunks[0],
    );

    let data = if app.power_draw_history_tenths_w.is_empty() {
        vec![0_u64]
    } else {
        app.power_draw_history_tenths_w.clone()
    };
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Power draw trend · 3m cadence"),
            )
            .data(&data)
            .style(theme::sparkline()),
        chunks[1],
    );
}

fn render_tlp_status<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let lines = if let Some(status) = &app.tlp_status {
        vec![
            Line::from(format!(
                "mode: {}",
                status.mode.as_deref().unwrap_or("unknown")
            )),
            Line::from(format!(
                "source: {}",
                status.power_source.as_deref().unwrap_or("unknown")
            )),
            Line::from(format!(
                "service: {}",
                status.service_state.as_deref().unwrap_or("unknown")
            )),
        ]
    } else if app.discovery.tlp.present {
        let profile_support = if app
            .tlp_version
            .as_ref()
            .is_some_and(jtop_core::TlpVersion::supports_named_profiles)
        {
            "named profiles ready"
        } else {
            "mode switching ready"
        };
        vec![
            Line::from("live status reads stay off the refresh path"),
            Line::from(profile_support),
            Line::from("use the action list for TLP changes"),
        ]
    } else {
        vec![Line::from("install tlp to enable mode/profile actions")]
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("TLP controls"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_powertop<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let lines = if let Some(report) = &app.powertop_report {
        vec![
            Line::from(format!("report window: {}s", report.seconds)),
            Line::from(format!("rows: {}", report.rows)),
            Line::from(format!("headers: {}", report.headers.join(", "))),
            Line::from("manual only, kept off the 3-minute live refresh path"),
        ]
    } else if !app.discovery.powertop.present {
        vec![Line::from(
            "powertop missing — install it to enable report generation",
        )]
    } else if !app.privilege.effective_root && !app.discovery.sudo.present {
        vec![Line::from("requires root/sudo for live report data")]
    } else {
        vec![Line::from("Press t to prepare a short powertop report.")]
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("Powertop"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_actions<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let selected = app.selected_action();
    let actions = app.power_actions();
    let items = if actions.is_empty() {
        vec![ListItem::new(
            "TLP actions unavailable — install tlp to enable commands",
        )]
    } else {
        actions
            .into_iter()
            .map(|action| {
                let prefix = if selected.as_ref() == Some(&action) {
                    "▶"
                } else {
                    " "
                };
                ListItem::new(format!("{prefix} {}", action.label()))
            })
            .collect::<Vec<_>>()
    };

    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Action list")),
        area,
    );
}

fn render_tooling<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let mut lines = vec![
        Line::from(format!("tlp: {}", presence(app.discovery.tlp.present))),
        Line::from(format!(
            "tlp-stat: {}",
            presence(app.discovery.tlp_stat.present)
        )),
        Line::from(format!(
            "powertop: {}",
            presence(app.discovery.powertop.present)
        )),
        Line::from(format!("sudo: {}", presence(app.discovery.sudo.present))),
    ];
    if let Some(version) = &app.tlp_version {
        lines.push(Line::from(format!(
            "tlp version: {}.{}.{}",
            version.major, version.minor, version.patch
        )));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("Discovery"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_config_preview<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let active = app
        .tlp_config_scan
        .as_ref()
        .map(|scan| {
            scan.files
                .iter()
                .filter(|file| matches!(file.state, TlpConfigState::ActiveConf))
                .count()
        })
        .unwrap_or(0);
    let disabled = app
        .tlp_config_scan
        .as_ref()
        .map(|scan| {
            scan.files
                .iter()
                .filter(|file| matches!(file.state, TlpConfigState::DisabledBak))
                .count()
        })
        .unwrap_or(0);
    let sample = app
        .tlp_config_scan
        .as_ref()
        .and_then(|scan| scan.files.iter().find(|file| !file.basename.is_empty()))
        .map(|file| file.basename.clone())
        .unwrap_or_else(|| "nothing scanned".into());

    let mut lines = vec![
        Line::from(format!("active snippets: {active}")),
        Line::from(format!("disabled snippets: {disabled}")),
        Line::from(format!("sample: {sample}")),
        Line::from("later this becomes the cool rename cannon"),
    ];

    if let Some(plan) = &app.tlp_config_preview {
        lines.push(Line::from(format!(
            "preview `{}` enables {} and disables {}",
            plan.target_set,
            plan.enable.len(),
            plan.disable.len()
        )));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("/etc/tlp.d preview"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_usage<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let lines = if let Some(usage) = &app.usage {
        let mut lines = vec![
            Line::from(format!(
                "cpu: {}",
                usage
                    .cpu_percent
                    .map(format_percent)
                    .unwrap_or_else(|| "n/a".into())
            )),
            Line::from(format!(
                "ram: {}",
                usage
                    .ram
                    .as_ref()
                    .map(|ram| format!(
                        "{} / {} ({})",
                        format_bytes(ram.used_bytes),
                        format_bytes(ram.total_bytes),
                        format_percent(ram.used_percent)
                    ))
                    .unwrap_or_else(|| "n/a".into())
            )),
        ];

        if usage.gpus.is_empty() {
            lines.push(Line::from("gpu: n/a"));
        } else {
            for gpu in &usage.gpus {
                lines.push(Line::from(format!(
                    "gpu: {} · {} · {} / {}",
                    gpu.name,
                    gpu.utilization_percent
                        .map(format_percent)
                        .unwrap_or_else(|| "n/a".into()),
                    gpu.memory_used_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "n/a".into()),
                    gpu.memory_total_bytes
                        .map(format_bytes)
                        .unwrap_or_else(|| "n/a".into())
                )));
            }
        }
        if !app.cpu_usage_history_percent.is_empty()
            || !app.ram_usage_history_percent.is_empty()
            || !app.gpu_usage_history_percent.is_empty()
        {
            lines.push(Line::from(format!(
                "trend: cpu {} · ram {} · gpu {}",
                compact_trend(&app.cpu_usage_history_percent),
                compact_trend(&app.ram_usage_history_percent),
                compact_trend(&app.gpu_usage_history_percent),
            )));
        }
        lines
    } else {
        vec![Line::from(
            "No system usage telemetry yet. Press r to refresh.",
        )]
    };

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("System usage"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_system_panels<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    if area.height < 26 && area.width >= 48 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[1]);

        render_actions(frame, left[0], app);
        render_usage(frame, left[1], app);
        render_tooling(frame, right[0], app);
        render_config_preview(frame, right[1], app);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(6),
        ])
        .split(area);

    render_actions(frame, chunks[0], app);
    render_usage(frame, chunks[1], app);
    render_tooling(frame, chunks[2], app);
    render_config_preview(frame, chunks[3], app);
}

fn render_logs<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let items = app
        .logs
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|line| ListItem::new(line.clone()))
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("Log")),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let line = if area.width >= 96 {
        "? help · r refresh now · p cycle action · t powertop report · !/Enter confirm · n cancel · q quit"
    } else if area.width >= 72 {
        "? help · r refresh · p cycle · t report · ! run · n cancel · q quit"
    } else {
        "? help · r ref · p cycle · t rpt · ! run · n cancel · q quit"
    };

    frame.render_widget(Paragraph::new(line).style(theme::help()), area);
}

fn render_confirmation<R: CommandRunner + Clone + Send + Sync + 'static>(
    frame: &mut Frame,
    area: Rect,
    app: &App<R>,
) {
    let spec = app.pending_spec().expect("pending spec required");
    let mode = match app.invocation_mode_for_execution(spec.needs_root) {
        jtop_core::InvocationMode::Direct => "direct",
        jtop_core::InvocationMode::AlreadyRoot => "already-root",
        jtop_core::InvocationMode::UseSudo => "sudo -n",
        jtop_core::InvocationMode::ReadOnlyOnly => "blocked",
    };
    let risk_style = match spec.risk {
        ActionRisk::Safe => None,
        ActionRisk::Caution => Some(theme::caution()),
        ActionRisk::Dangerous => Some(theme::danger()),
    };
    let lines = vec![
        Line::from(
            app.pending_action_label()
                .unwrap_or_else(|| "pending action".into()),
        )
        .bold(),
        Line::from(vec![
            Span::raw("risk: "),
            Span::styled(spec.risk.label(), risk_style.unwrap_or_default()),
        ]),
        Line::from(format!("mode: {mode}")),
        Line::from(format!("command: {} {}", spec.program, spec.args.join(" "))),
        Line::from("Press ! or Enter to execute, n to cancel."),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Confirm action"),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from("? toggle help"),
        Line::from("r refresh discovery and live battery status now"),
        Line::from("p cycle TLP action"),
        Line::from("t prepare powertop report"),
        Line::from("! / Enter confirm or execute"),
        Line::from("n cancel confirmation"),
        Line::from("battery telemetry auto-refreshes every 3 minutes"),
        Line::from("q / Esc / Ctrl-C quit"),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn confirmation_area(size: Rect, visible: bool) -> Option<Rect> {
    if !visible {
        return None;
    }
    Some(Rect {
        x: size.width / 8,
        y: size.height / 5,
        width: size.width.saturating_mul(3) / 4,
        height: size.height.saturating_mul(3) / 5,
    })
}

fn presence(present: bool) -> &'static str {
    if present { "ready" } else { "missing" }
}

fn render_battery_line(battery: &BatterySummary) -> Line<'static> {
    let percent = battery.capacity_percent.unwrap_or(0);
    let mut spans = vec![Span::styled(
        format!("{:<5}", battery.name),
        Style::default().fg(Color::Cyan),
    )];
    spans.extend(segmented_battery_bar(percent, 18));
    spans.push(Span::raw(format!(" {:>3}%", percent)));
    if let Some(power) = battery.power_now_w {
        spans.push(Span::raw(format!(" · {:>4.1}W", power)));
    }
    if let Some(hours) = battery.hours_remaining.or(battery.hours_to_full) {
        spans.push(Span::raw(format!(" · {}", format_hours(hours))));
    }
    Line::from(spans)
}

fn segmented_battery_bar(percent: u8, segments: usize) -> Vec<Span<'static>> {
    let filled = (usize::from(percent) * segments).div_ceil(100);
    (0..segments)
        .map(|index| {
            if index < filled {
                Span::styled("█", gradient_style(index, segments))
            } else {
                Span::styled("░", theme::empty_bar())
            }
        })
        .collect()
}

fn gradient_style(index: usize, segments: usize) -> Style {
    let bucket = (index * 5) / segments.max(1);
    match bucket {
        0 => theme::battery_red(),
        1 => theme::battery_orange(),
        2 => theme::battery_yellow(),
        3 => theme::battery_lime(),
        _ => theme::battery_green(),
    }
}

fn battery_life_label(fleet: &BatteryFleetSummary) -> String {
    if let Some(hours) = fleet.hours_remaining {
        format!("{} left", format_hours(hours))
    } else if let Some(hours) = fleet.hours_to_full {
        format!("{} to full", format_hours(hours))
    } else {
        "life estimate n/a".into()
    }
}

fn battery_status_label(status: Option<&str>) -> &'static str {
    match status {
        Some("charging") => "charging",
        Some("discharging") => "discharging",
        Some("full") => "full",
        Some("mixed") => "mixed",
        Some("not charging") => "idle",
        _ => "status unknown",
    }
}

fn format_hours(hours: f64) -> String {
    let total_minutes = (hours.max(0.0) * 60.0).round() as u64;
    let whole_hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    format!("{whole_hours}h {minutes:02}m")
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s ago")
    } else {
        format!("{}m ago", seconds / 60)
    }
}

fn format_percent(value: f64) -> String {
    format!("{value:.1}%")
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else {
        format!("{bytes} B")
    }
}

fn compact_trend(history: &[u64]) -> String {
    match (history.first(), history.last()) {
        (Some(first), Some(last)) if history.len() > 1 => {
            let delta = *last as i64 - *first as i64;
            format!("{last}% ({delta:+})")
        }
        (Some(value), _) => format!("{value}%"),
        _ => "n/a".into(),
    }
}

fn average_health(fleet: &BatteryFleetSummary) -> Option<f64> {
    let values = fleet
        .batteries
        .iter()
        .filter_map(|battery| battery.health_percent)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values.iter().sum::<f64>() / values.len() as f64)
}

fn cycle_summary(fleet: &BatteryFleetSummary) -> String {
    let cycles = fleet
        .batteries
        .iter()
        .filter_map(|battery| battery.cycle_count)
        .collect::<Vec<_>>();
    match cycles.as_slice() {
        [] => "n/a".into(),
        [single] => single.to_string(),
        values => format!(
            "{}-{}",
            values.iter().min().unwrap(),
            values.iter().max().unwrap()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use jtop_core::{
        BatteryFleetSummary, BatterySummary, DiscoveryReport, FakeCommandRunner, PrivilegeState,
        ToolStatus,
    };
    use ratatui::{Terminal, backend::TestBackend};

    fn discovery() -> DiscoveryReport {
        DiscoveryReport {
            tlp: ToolStatus {
                name: "tlp",
                present: true,
                hint: "",
            },
            tlp_stat: ToolStatus {
                name: "tlp-stat",
                present: true,
                hint: "",
            },
            powertop: ToolStatus {
                name: "powertop",
                present: false,
                hint: "",
            },
            sudo: ToolStatus {
                name: "sudo",
                present: false,
                hint: "",
            },
        }
    }

    #[test]
    fn renders_key_labels() {
        let backend = TestBackend::new(120, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(
            FakeCommandRunner::default(),
            PrivilegeState {
                effective_root: false,
                sudo_available: false,
            },
            discovery(),
        );
        app.battery = Some(BatteryFleetSummary {
            batteries: vec![BatterySummary {
                name: "BAT0".into(),
                path: std::path::PathBuf::from("/sys/class/power_supply/BAT0"),
                status: Some("Discharging".into()),
                capacity_percent: Some(76),
                energy_now_wh: Some(42.0),
                energy_full_wh: Some(55.0),
                energy_full_design_wh: Some(60.0),
                power_now_w: Some(11.8),
                voltage_now_v: Some(12.1),
                cycle_count: Some(188),
                health_percent: Some(91.7),
                hours_remaining: Some(3.6),
                hours_to_full: None,
            }],
            aggregate_status: Some("discharging".into()),
            total_capacity_percent: Some(76),
            total_energy_now_wh: Some(42.0),
            total_energy_full_wh: Some(55.0),
            total_power_now_w: Some(11.8),
            hours_remaining: Some(3.6),
            hours_to_full: None,
        });
        app.power_draw_history_tenths_w = vec![95, 101, 120, 118];
        app.cpu_usage_history_percent = vec![38, 43];
        app.ram_usage_history_percent = vec![60, 63];
        app.gpu_usage_history_percent = vec![70, 71];
        app.usage = Some(jtop_core::UsageSummary {
            cpu_percent: Some(42.5),
            ram: Some(jtop_core::RamUsage {
                total_bytes: 16 * 1024 * 1024 * 1024,
                available_bytes: Some(6 * 1024 * 1024 * 1024),
                used_bytes: 10 * 1024 * 1024 * 1024,
                used_percent: 62.5,
            }),
            gpus: vec![jtop_core::GpuUsage {
                name: "GPU0".into(),
                utilization_percent: Some(71.0),
                memory_used_bytes: Some(3 * 1024 * 1024 * 1024),
                memory_total_bytes: Some(8 * 1024 * 1024 * 1024),
            }],
        });

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Battery deck"));
        assert!(rendered.contains("Live telemetry"));
        assert!(rendered.contains("BAT0"));
        assert!(rendered.contains("76%"));
        assert!(rendered.contains("Action list"));
        assert!(rendered.contains("System usage"));
        assert!(rendered.contains("cpu: 42.5%"));
        assert!(rendered.contains("ram: 10.0 GiB"));
        assert!(rendered.contains("gpu: GPU0"));
        assert!(rendered.contains("71.0%"));
        assert!(rendered.contains("trend: cpu 43% (+5)"));
        assert!(rendered.contains("Powertop"));
    }
}
