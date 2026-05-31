use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use jtop_core::{
    ActionContext, BatteryFleetSummary, CommandRunner, DEFAULT_POWER_SUPPLY_DIR, DEFAULT_PROC_DIR,
    DEFAULT_SYS_DIR, DiscoveryReport, Error, InvocationMode, PowerAction, PreparedPowertopReport,
    PrivilegeState, StdCommandRunner, StdSudoProbe, StdToolLocator, TlpConfigScan,
    TlpConfigSwitchPlan, TlpMode, TlpProfile, TlpVersion, detect_privilege_state, discover_tools,
    invocation_mode, plan_tlp_config_switch, read_battery_fleet, read_cpu_times, read_tlp_version,
    read_usage_summary_with_cpu_times, scan_tlp_config_dir,
};
use ratatui::{Terminal, backend::CrosstermBackend};
use signal_hook::{consts::SIGINT, flag};

use crate::{input::AppEvent, input::map_key_event, ui};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_PROBE_TIMEOUT: Duration = Duration::from_secs(1);
const POWERTOP_SAMPLE_SECONDS: u64 = 5;
const POWERTOP_TIMEOUT_GRACE: Duration = Duration::from_secs(15);
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
const LOG_LIMIT: usize = 200;
const HISTORY_LIMIT: usize = 24;

#[derive(Debug)]
enum PendingAction {
    Standard {
        action: PowerAction,
        spec: jtop_core::CommandSpec,
    },
    Powertop(PreparedPowertopReport),
}

impl PendingAction {
    fn action(&self) -> PowerAction {
        match self {
            Self::Standard { action, .. } => action.clone(),
            Self::Powertop(prepared) => PowerAction::PowertopReport {
                seconds: prepared
                    .spec()
                    .args
                    .iter()
                    .find_map(|arg| arg.strip_prefix("--time="))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(POWERTOP_SAMPLE_SECONDS),
            },
        }
    }

    fn spec(&self) -> &jtop_core::CommandSpec {
        match self {
            Self::Standard { spec, .. } => spec,
            Self::Powertop(prepared) => prepared.spec(),
        }
    }
}

#[derive(Debug)]
enum BackgroundResult {
    Refresh(RefreshSnapshot),
    Execution(ExecutionSnapshot),
}

#[derive(Debug)]
struct RefreshSnapshot {
    discovery: DiscoveryReport,
    privilege: PrivilegeState,
    battery: Option<BatteryFleetSummary>,
    usage: Option<jtop_core::UsageSummary>,
    previous_cpu_times: Option<jtop_core::CpuTimes>,
    tlp_version: Option<TlpVersion>,
    tlp_status: Option<jtop_core::TlpStatusSummary>,
    tlp_config: Option<jtop_core::TlpConfigSummary>,
    powertop_version: Option<jtop_core::PowertopVersion>,
    tlp_config_scan: Option<TlpConfigScan>,
    tlp_config_preview: Option<TlpConfigSwitchPlan>,
    selected_config_target: Option<String>,
    logs: Vec<String>,
}

#[derive(Debug)]
struct ExecutionSnapshot {
    powertop_report: Option<jtop_core::PowertopReportSummary>,
    refresh: Option<RefreshSnapshot>,
    logs: Vec<String>,
}

pub struct App<R: CommandRunner + Clone + Send + Sync + 'static> {
    pub runner: R,
    pub should_quit: bool,
    pub show_help: bool,
    pub privilege: PrivilegeState,
    pub discovery: DiscoveryReport,
    pub battery: Option<BatteryFleetSummary>,
    pub usage: Option<jtop_core::UsageSummary>,
    pub tlp_version: Option<TlpVersion>,
    pub tlp_status: Option<jtop_core::TlpStatusSummary>,
    pub tlp_config: Option<jtop_core::TlpConfigSummary>,
    pub powertop_version: Option<jtop_core::PowertopVersion>,
    pub powertop_report: Option<jtop_core::PowertopReportSummary>,
    pub tlp_config_scan: Option<TlpConfigScan>,
    pub tlp_config_preview: Option<TlpConfigSwitchPlan>,
    pub selected_power_action: usize,
    pub logs: Vec<String>,
    pub busy_message: Option<String>,
    pub power_draw_history_tenths_w: Vec<u64>,
    pub charge_history_percent: Vec<u64>,
    pub cpu_usage_history_percent: Vec<u64>,
    pub ram_usage_history_percent: Vec<u64>,
    pub gpu_usage_history_percent: Vec<u64>,
    pub last_refresh_completed_at: Option<Instant>,
    pub previous_cpu_times: Option<jtop_core::CpuTimes>,
    pending_action: Option<PendingAction>,
    selected_config_target: Option<String>,
    background_rx: Option<Receiver<BackgroundResult>>,
}

impl<R: CommandRunner + Clone + Send + Sync + 'static> App<R> {
    pub fn new(runner: R, privilege: PrivilegeState, discovery: DiscoveryReport) -> Self {
        Self {
            runner,
            should_quit: false,
            show_help: false,
            privilege,
            discovery,
            battery: None,
            usage: None,
            tlp_version: None,
            tlp_status: None,
            tlp_config: None,
            powertop_version: None,
            powertop_report: None,
            tlp_config_scan: None,
            tlp_config_preview: None,
            selected_power_action: 0,
            logs: vec!["jtop booted".into()],
            busy_message: None,
            power_draw_history_tenths_w: Vec::new(),
            charge_history_percent: Vec::new(),
            cpu_usage_history_percent: Vec::new(),
            ram_usage_history_percent: Vec::new(),
            gpu_usage_history_percent: Vec::new(),
            last_refresh_completed_at: None,
            previous_cpu_times: None,
            pending_action: None,
            selected_config_target: None,
            background_rx: None,
        }
    }

    pub fn maybe_start_auto_refresh(&mut self) {
        if self.background_rx.is_some() {
            return;
        }

        let should_refresh = self
            .last_refresh_completed_at
            .is_none_or(|last| last.elapsed() >= AUTO_REFRESH_INTERVAL);
        if should_refresh {
            self.start_refresh_with_message("refreshing live battery + power stats".into());
        }
    }

    pub fn power_actions(&self) -> Vec<PowerAction> {
        if !self.discovery.tlp.present {
            return Vec::new();
        }

        let mut actions = vec![
            PowerAction::TlpMode(TlpMode::Ac),
            PowerAction::TlpMode(TlpMode::Bat),
        ];
        if self
            .tlp_version
            .as_ref()
            .is_some_and(TlpVersion::supports_named_profiles)
        {
            actions.extend([
                PowerAction::TlpNamedProfile(TlpProfile::Balanced),
                PowerAction::TlpNamedProfile(TlpProfile::Performance),
                PowerAction::TlpNamedProfile(TlpProfile::PowerSaver),
            ]);
        }
        actions
    }

    pub fn selected_action(&self) -> Option<PowerAction> {
        let actions = self.power_actions();
        if actions.is_empty() {
            None
        } else {
            Some(actions[self.selected_power_action % actions.len()].clone())
        }
    }

    pub fn pending_spec(&self) -> Option<&jtop_core::CommandSpec> {
        self.pending_action.as_ref().map(PendingAction::spec)
    }

    pub fn pending_action_label(&self) -> Option<String> {
        self.pending_action
            .as_ref()
            .map(|pending| pending.action().label())
    }

    pub fn invocation_mode_for_execution(&self, needs_root: bool) -> InvocationMode {
        execution_invocation_mode(
            invocation_mode(needs_root, &self.privilege),
            &self.discovery,
        )
    }

    pub fn poll_background(&mut self) {
        let Some(receiver) = self.background_rx.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(BackgroundResult::Refresh(snapshot)) => {
                self.busy_message = None;
                self.apply_refresh_snapshot(snapshot);
            }
            Ok(BackgroundResult::Execution(snapshot)) => {
                self.busy_message = None;
                self.apply_execution_snapshot(snapshot);
            }
            Err(TryRecvError::Empty) => {
                self.background_rx = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                self.busy_message = None;
                self.push_log("background task disconnected".into());
            }
        }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        if self.show_help {
            match event {
                AppEvent::Quit => {
                    self.quit();
                    return;
                }
                AppEvent::ToggleHelp => {
                    self.show_help = false;
                    return;
                }
                _ => return,
            }
        }

        match event {
            AppEvent::Quit => self.quit(),
            AppEvent::ToggleHelp => self.show_help = !self.show_help,
            AppEvent::Refresh => self.start_refresh_with_message("refreshing system status".into()),
            AppEvent::CyclePowerAction => self.cycle_power_action(),
            AppEvent::PreparePowertopReport => self.prepare_powertop_report(),
            AppEvent::ConfirmOrExecute => self.confirm_or_execute(),
            AppEvent::CancelConfirmation => {
                if self.pending_action.take().is_some() {
                    self.push_log("cancelled pending action".into());
                }
            }
            AppEvent::None => {}
        }
    }

    fn cycle_power_action(&mut self) {
        let len = self.power_actions().len();
        if len == 0 {
            self.push_log("TLP actions unavailable: install tlp to enable mode switching".into());
            return;
        }

        self.selected_power_action = (self.selected_power_action + 1) % len;
        if let Some(action) = self.selected_action() {
            self.push_log(format!("selected {}", action.label()));
        }
    }

    fn confirm_or_execute(&mut self) {
        if self.pending_action.is_some() {
            self.start_execute_pending();
            return;
        }

        let Some(action) = self.selected_action() else {
            self.push_log("no TLP action available to confirm".into());
            return;
        };

        let spec = match action.command_spec(&ActionContext {
            tlp_version: self.tlp_version.clone(),
            powertop_csv_path: None,
        }) {
            Ok(spec) => spec,
            Err(error) => {
                self.push_log(error.to_string());
                return;
            }
        };
        self.pending_action = Some(PendingAction::Standard {
            action: action.clone(),
            spec,
        });
        self.push_log(format!("confirm {} with ! or Enter", action.label()));
    }

    fn prepare_powertop_report(&mut self) {
        if !self.discovery.powertop.present {
            self.push_log("powertop missing — install powertop to enable report generation".into());
            return;
        }

        match PreparedPowertopReport::new(POWERTOP_SAMPLE_SECONDS) {
            Ok(prepared) => {
                let sample_seconds = powertop_sample_seconds(prepared.spec());
                let timeout = powertop_timeout(prepared.spec());
                self.pending_action = Some(PendingAction::Powertop(prepared));
                self.push_log(format!(
                    "powertop report prepared ({sample_seconds}s sample window, up to {}s runtime); confirm with ! or Enter",
                    timeout.as_secs()
                ));
            }
            Err(error) => self.push_log(error.to_string()),
        }
    }

    fn start_refresh_with_message(&mut self, busy_message: String) {
        if self.background_rx.is_some() {
            self.push_log_once("background task already running".into());
            return;
        }

        let (tx, rx) = mpsc::channel();
        let selected_config_target = self.selected_config_target.clone();
        let previous_cpu_times = self.previous_cpu_times.clone();
        let tlp_version = self.tlp_version.clone();
        let privilege = self.privilege.clone();
        self.busy_message = Some(busy_message);
        self.background_rx = Some(rx);

        thread::spawn(move || {
            let snapshot = collect_refresh_snapshot(
                selected_config_target,
                previous_cpu_times,
                tlp_version,
                privilege,
            );
            let _ = tx.send(BackgroundResult::Refresh(snapshot));
        });
    }

    fn start_execute_pending(&mut self) {
        if self.background_rx.is_some() {
            self.push_log_once("background task already running".into());
            return;
        }

        let Some(mode) = self
            .pending_action
            .as_ref()
            .map(|pending| self.invocation_mode_for_execution(pending.spec().needs_root))
        else {
            return;
        };
        if matches!(mode, InvocationMode::ReadOnlyOnly) {
            self.push_log("privileged action blocked: install sudo or launch jtop as root".into());
            return;
        }

        let Some(pending) = self.pending_action.take() else {
            return;
        };

        let (tx, rx) = mpsc::channel();
        let runner = self.runner.clone();
        let selected_config_target = self.selected_config_target.clone();
        let previous_cpu_times = self.previous_cpu_times.clone();
        let tlp_version = self.tlp_version.clone();
        let privilege = self.privilege.clone();
        let action_label = pending.action().label();
        self.busy_message = Some(format!("running {action_label}"));
        self.background_rx = Some(rx);

        thread::spawn(move || {
            let snapshot = collect_execution_snapshot(
                runner,
                pending,
                mode,
                selected_config_target,
                previous_cpu_times,
                tlp_version,
                privilege,
            );
            let _ = tx.send(BackgroundResult::Execution(snapshot));
        });
    }

    fn apply_refresh_snapshot(&mut self, snapshot: RefreshSnapshot) {
        self.discovery = snapshot.discovery;
        self.privilege = snapshot.privilege;
        self.battery = snapshot.battery;
        self.usage = snapshot.usage;
        self.previous_cpu_times = snapshot.previous_cpu_times;
        self.tlp_version = snapshot.tlp_version;
        self.tlp_status = snapshot.tlp_status;
        self.tlp_config = snapshot.tlp_config;
        self.powertop_version = snapshot.powertop_version;
        self.tlp_config_scan = snapshot.tlp_config_scan;
        self.tlp_config_preview = snapshot.tlp_config_preview;
        self.selected_config_target = snapshot.selected_config_target;
        self.last_refresh_completed_at = Some(Instant::now());
        self.push_battery_history();
        self.push_usage_history();
        for message in snapshot.logs {
            self.push_log_once(message);
        }

        let action_count = self.power_actions().len();
        self.selected_power_action = if action_count == 0 {
            0
        } else {
            self.selected_power_action % action_count
        };
    }

    fn apply_execution_snapshot(&mut self, snapshot: ExecutionSnapshot) {
        if let Some(report) = snapshot.powertop_report {
            self.powertop_report = Some(report);
        }
        if let Some(refresh) = snapshot.refresh {
            self.apply_refresh_snapshot(refresh);
        }
        for message in snapshot.logs {
            self.push_log(message);
        }
    }

    fn push_log(&mut self, message: String) {
        self.logs.push(message);
        if self.logs.len() > LOG_LIMIT {
            let excess = self.logs.len() - LOG_LIMIT;
            self.logs.drain(0..excess);
        }
    }

    fn push_log_once(&mut self, message: String) {
        if !self.logs.iter().any(|existing| existing == &message) {
            self.push_log(message);
        }
    }

    fn push_battery_history(&mut self) {
        let Some(battery) = &self.battery else {
            return;
        };

        if let Some(charge) = battery.total_capacity_percent {
            push_history_point(&mut self.charge_history_percent, u64::from(charge));
        }
        if let Some(power) = battery.total_power_now_w {
            push_history_point(
                &mut self.power_draw_history_tenths_w,
                (power * 10.0).round() as u64,
            );
        }
    }

    fn push_usage_history(&mut self) {
        let Some(usage) = &self.usage else {
            return;
        };

        if let Some(cpu_percent) = usage.cpu_percent {
            push_history_point(
                &mut self.cpu_usage_history_percent,
                percent_history_value(cpu_percent),
            );
        }
        if let Some(ram) = &usage.ram {
            push_history_point(
                &mut self.ram_usage_history_percent,
                percent_history_value(ram.used_percent),
            );
        }
        if let Some(gpu_percent) = usage
            .gpus
            .iter()
            .filter_map(|gpu| gpu.utilization_percent)
            .reduce(f64::max)
        {
            push_history_point(
                &mut self.gpu_usage_history_percent,
                percent_history_value(gpu_percent),
            );
        }
    }

    pub fn quit(&mut self) {
        self.runner.request_shutdown();
        self.should_quit = true;
    }
}

impl App<StdCommandRunner> {
    fn bootstrap() -> Self {
        let discovery = discover_tools(&StdToolLocator);
        let privilege =
            detect_privilege_state(&StdSudoProbe, discovery.sudo.present, STARTUP_PROBE_TIMEOUT)
                .unwrap_or(PrivilegeState {
                    effective_root: false,
                    sudo_available: false,
                });
        let mut app = Self::new(StdCommandRunner::default(), privilege, discovery);
        if app.discovery.tlp.present {
            match read_tlp_version(&app.runner, STARTUP_PROBE_TIMEOUT) {
                Ok(version) => app.tlp_version = Some(version),
                Err(error) => app.push_log_once(format!("TLP version probe: {error}")),
            }
        }
        app.start_refresh_with_message("refreshing system status".into());
        app
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::bootstrap();
    let sigint = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, Arc::clone(&sigint))?;

    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    while !app.should_quit {
        app.poll_background();
        app.maybe_start_auto_refresh();
        terminal.draw(|frame| ui::render(frame, &app))?;

        if sigint.load(Ordering::Relaxed) {
            app.quit();
            continue;
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            app.handle_event(map_key_event(key));
        }
    }

    app.runner.request_shutdown();
    Ok(())
}

fn collect_refresh_snapshot(
    selected_config_target: Option<String>,
    previous_cpu_times: Option<jtop_core::CpuTimes>,
    existing_tlp_version: Option<TlpVersion>,
    existing_privilege: PrivilegeState,
) -> RefreshSnapshot {
    let discovery = discover_tools(&StdToolLocator);
    let privilege = existing_privilege;

    let mut logs = Vec::new();

    let current_cpu_times = match read_cpu_times(DEFAULT_PROC_DIR) {
        Ok(cpu_times) => Some(cpu_times),
        Err(error) => {
            logs.push(format!("usage telemetry: {error}"));
            None
        }
    };

    let battery = match read_battery_fleet(DEFAULT_POWER_SUPPLY_DIR) {
        Ok(summary) => Some(summary),
        Err(error) => {
            logs.push(format!("battery telemetry: {error}"));
            None
        }
    };

    let usage = if let Some(cpu_times) = current_cpu_times.as_ref() {
        match read_usage_summary_with_cpu_times(
            DEFAULT_PROC_DIR,
            DEFAULT_SYS_DIR,
            previous_cpu_times.as_ref(),
            cpu_times,
        ) {
            Ok(summary) => Some(summary),
            Err(error) => {
                logs.push(format!("usage telemetry: {error}"));
                None
            }
        }
    } else {
        None
    };

    let tlp_version = existing_tlp_version;

    let (tlp_config_scan, tlp_config_preview, selected_config_target) = if discovery.tlp.present
        || discovery.tlp_stat.present
    {
        match scan_tlp_config_dir(jtop_core::tlp_config::DEFAULT_TLP_DIR) {
            Ok(scan) => {
                let targets = config_set_names(&scan);
                let selected = selected_config_target
                    .filter(|target| targets.iter().any(|candidate| candidate == target))
                    .or_else(|| targets.first().cloned());
                let preview = selected.as_ref().and_then(|target| {
                    match plan_tlp_config_switch(&scan, target) {
                        Ok(plan) => Some(plan),
                        Err(error) => {
                            logs.push(format!("TLP snippet preview: {error}"));
                            None
                        }
                    }
                });
                (Some(scan), preview, selected)
            }
            Err(error) => {
                logs.push(format!("TLP snippets: {error}"));
                (None, None, None)
            }
        }
    } else {
        (None, None, None)
    };

    logs.extend(discovery.missing_messages());

    RefreshSnapshot {
        discovery,
        privilege,
        battery,
        usage,
        previous_cpu_times: current_cpu_times,
        tlp_version,
        tlp_status: None,
        tlp_config: None,
        powertop_version: None,
        tlp_config_scan,
        tlp_config_preview,
        selected_config_target,
        logs,
    }
}

fn execution_invocation_mode(mode: InvocationMode, discovery: &DiscoveryReport) -> InvocationMode {
    match mode {
        InvocationMode::ReadOnlyOnly if discovery.sudo.present => InvocationMode::UseSudo,
        other => other,
    }
}

fn push_history_point(history: &mut Vec<u64>, value: u64) {
    history.push(value);
    if history.len() > HISTORY_LIMIT {
        let excess = history.len() - HISTORY_LIMIT;
        history.drain(0..excess);
    }
}

fn percent_history_value(value: f64) -> u64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0).round() as u64
    } else {
        0
    }
}

fn collect_execution_snapshot<R: CommandRunner>(
    runner: R,
    pending: PendingAction,
    mode: InvocationMode,
    selected_config_target: Option<String>,
    previous_cpu_times: Option<jtop_core::CpuTimes>,
    tlp_version: Option<TlpVersion>,
    privilege: PrivilegeState,
) -> ExecutionSnapshot {
    match pending {
        PendingAction::Standard { action, spec } => {
            match runner.run(&spec, mode, COMMAND_TIMEOUT) {
                Ok(output) => {
                    let mut logs = vec![format!("executed {}", action.label())];
                    if !output.stdout.trim().is_empty() {
                        logs.push(output.stdout.trim().to_string());
                    }
                    if !output.stderr.trim().is_empty() {
                        logs.push(output.stderr.trim().to_string());
                    }
                    ExecutionSnapshot {
                        powertop_report: None,
                        refresh: Some(collect_refresh_snapshot(
                            selected_config_target,
                            previous_cpu_times,
                            tlp_version,
                            privilege,
                        )),
                        logs,
                    }
                }
                Err(error) => ExecutionSnapshot {
                    powertop_report: None,
                    refresh: None,
                    logs: vec![error.to_string()],
                },
            }
        }
        PendingAction::Powertop(prepared) => {
            let sample_seconds = powertop_sample_seconds(prepared.spec());
            let timeout = powertop_timeout(prepared.spec());
            match runner.run(prepared.spec(), mode, timeout) {
                Ok(_) => match prepared.finish() {
                    Ok(report) => ExecutionSnapshot {
                        powertop_report: Some(report),
                        refresh: None,
                        logs: vec!["powertop report captured".into()],
                    },
                    Err(error) => ExecutionSnapshot {
                        powertop_report: None,
                        refresh: None,
                        logs: vec![error.to_string()],
                    },
                },
                Err(Error::CommandTimeout { .. }) => ExecutionSnapshot {
                    powertop_report: None,
                    refresh: None,
                    logs: vec![format!(
                        "powertop report timed out after {}s while capturing a {sample_seconds}s sample window",
                        timeout.as_secs()
                    )],
                },
                Err(error) => ExecutionSnapshot {
                    powertop_report: None,
                    refresh: None,
                    logs: vec![error.to_string()],
                },
            }
        }
    }
}

fn powertop_sample_seconds(spec: &jtop_core::CommandSpec) -> u64 {
    spec.args
        .iter()
        .find_map(|arg| arg.strip_prefix("--time="))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(POWERTOP_SAMPLE_SECONDS)
}

fn powertop_timeout(spec: &jtop_core::CommandSpec) -> Duration {
    Duration::from_secs(powertop_sample_seconds(spec)).saturating_add(POWERTOP_TIMEOUT_GRACE)
}

fn config_set_names(scan: &TlpConfigScan) -> Vec<String> {
    let mut names = BTreeSet::new();
    for file in &scan.files {
        if matches!(
            file.state,
            jtop_core::TlpConfigState::ActiveConf | jtop_core::TlpConfigState::DisabledBak
        ) && let Some((_, set_name)) = file.basename.rsplit_once('@')
        {
            names.insert(set_name.to_string());
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jtop_core::{CommandOutput, FakeCommandRunner, ToolStatus};
    use std::{thread, time::Instant};

    fn test_discovery() -> DiscoveryReport {
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
                present: true,
                hint: "",
            },
            sudo: ToolStatus {
                name: "sudo",
                present: true,
                hint: "",
            },
        }
    }

    fn test_discovery_without_sudo() -> DiscoveryReport {
        DiscoveryReport {
            sudo: ToolStatus {
                name: "sudo",
                present: false,
                hint: "",
            },
            ..test_discovery()
        }
    }

    fn wait_for_background<R: CommandRunner + Clone + Send + Sync + 'static>(app: &mut App<R>) {
        let started = Instant::now();
        while app.background_rx.is_some() {
            app.poll_background();
            if started.elapsed() > Duration::from_secs(1) {
                panic!("background task did not finish in time");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn risky_actions_wait_for_confirmation() {
        let runner = FakeCommandRunner::default();
        let mut app = App::new(
            runner.clone(),
            PrivilegeState {
                effective_root: false,
                sudo_available: true,
            },
            test_discovery(),
        );
        app.tlp_version = Some(TlpVersion::new(1, 9, 0));
        app.handle_event(AppEvent::ConfirmOrExecute);
        assert_eq!(runner.calls().len(), 0);
        assert!(app.pending_spec().is_some());
    }

    #[test]
    fn execution_happens_after_confirmation() {
        let runner = FakeCommandRunner::default();
        runner.push_response(Ok(CommandOutput {
            stdout: "done".into(),
            stderr: String::new(),
            exit_code: 0,
        }));
        let mut app = App::new(
            runner.clone(),
            PrivilegeState {
                effective_root: false,
                sudo_available: true,
            },
            test_discovery(),
        );
        app.handle_event(AppEvent::ConfirmOrExecute);
        app.handle_event(AppEvent::ConfirmOrExecute);
        wait_for_background(&mut app);
        assert!(!runner.calls().is_empty());
        assert_eq!(runner.calls()[0].program, "tlp");
    }

    #[test]
    fn help_overlay_blocks_hidden_execution() {
        let runner = FakeCommandRunner::default();
        let mut app = App::new(
            runner.clone(),
            PrivilegeState {
                effective_root: false,
                sudo_available: true,
            },
            test_discovery(),
        );
        app.handle_event(AppEvent::ConfirmOrExecute);
        app.handle_event(AppEvent::ToggleHelp);
        app.handle_event(AppEvent::ConfirmOrExecute);
        assert_eq!(runner.calls().len(), 0);
        assert!(app.pending_spec().is_some());
    }

    #[test]
    fn blocked_privileged_execution_keeps_pending_confirmation() {
        let runner = FakeCommandRunner::default();
        let mut app = App::new(
            runner,
            PrivilegeState {
                effective_root: false,
                sudo_available: false,
            },
            test_discovery_without_sudo(),
        );
        app.handle_event(AppEvent::ConfirmOrExecute);
        app.handle_event(AppEvent::ConfirmOrExecute);
        assert!(app.pending_spec().is_some());
    }

    #[test]
    fn confirmed_action_tries_sudo_when_ticket_status_is_stale() {
        let runner = FakeCommandRunner::default();
        runner.push_response(Ok(CommandOutput {
            stdout: "done".into(),
            stderr: String::new(),
            exit_code: 0,
        }));
        let mut app = App::new(
            runner.clone(),
            PrivilegeState {
                effective_root: false,
                sudo_available: false,
            },
            test_discovery(),
        );

        app.handle_event(AppEvent::ConfirmOrExecute);
        app.handle_event(AppEvent::ConfirmOrExecute);
        wait_for_background(&mut app);

        assert_eq!(runner.calls()[0].mode, InvocationMode::UseSudo);
    }

    #[test]
    fn powertop_timeout_includes_startup_slack() {
        let prepared = PreparedPowertopReport::new(POWERTOP_SAMPLE_SECONDS).unwrap();
        assert_eq!(
            powertop_sample_seconds(prepared.spec()),
            POWERTOP_SAMPLE_SECONDS
        );
        assert_eq!(powertop_timeout(prepared.spec()), Duration::from_secs(20));
    }

    #[test]
    fn powertop_timeout_falls_back_for_missing_time_arg() {
        let spec = jtop_core::CommandSpec {
            program: "powertop".into(),
            args: vec!["--csv=/tmp/test.csv".into()],
            needs_root: true,
            risk: jtop_core::ActionRisk::Caution,
            description: "Generate powertop CSV report".into(),
        };

        assert_eq!(powertop_sample_seconds(&spec), POWERTOP_SAMPLE_SECONDS);
        assert_eq!(powertop_timeout(&spec), Duration::from_secs(20));
    }

    #[test]
    fn powertop_preparation_log_mentions_sample_and_runtime() {
        let runner = FakeCommandRunner::default();
        let mut app = App::new(
            runner,
            PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            test_discovery(),
        );

        app.handle_event(AppEvent::PreparePowertopReport);

        assert!(app.pending_spec().is_some());
        let last_log = app.logs.last().unwrap();
        assert!(last_log.contains("5s sample window"));
        assert!(last_log.contains("20s runtime"));
    }

    #[test]
    fn powertop_timeouts_are_reported_with_context() {
        let runner = FakeCommandRunner::default();
        runner.push_response(Err(Error::CommandTimeout {
            program: "powertop".into(),
            timeout: Duration::from_secs(20),
        }));
        let mut app = App::new(
            runner.clone(),
            PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            test_discovery(),
        );

        app.handle_event(AppEvent::PreparePowertopReport);
        app.handle_event(AppEvent::ConfirmOrExecute);
        wait_for_background(&mut app);

        assert_eq!(runner.calls()[0].timeout, Duration::from_secs(20));
        let last_log = app.logs.last().unwrap();
        assert!(last_log.contains("powertop report timed out after 20s"));
        assert!(last_log.contains("5s sample window"));
    }

    #[test]
    fn auto_refresh_starts_after_refresh_window_elapses() {
        let runner = FakeCommandRunner::default();
        let mut app = App::new(
            runner,
            PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            test_discovery(),
        );

        app.last_refresh_completed_at =
            Some(Instant::now() - AUTO_REFRESH_INTERVAL - Duration::from_secs(1));
        app.maybe_start_auto_refresh();

        assert!(app.background_rx.is_some());
        assert_eq!(
            app.busy_message.as_deref(),
            Some("refreshing live battery + power stats")
        );
    }

    #[test]
    fn battery_history_tracks_charge_and_power() {
        let mut app = App::new(
            FakeCommandRunner::default(),
            PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            test_discovery(),
        );
        app.battery = Some(BatteryFleetSummary {
            batteries: Vec::new(),
            aggregate_status: Some("discharging".into()),
            total_capacity_percent: Some(63),
            total_energy_now_wh: Some(31.0),
            total_energy_full_wh: Some(49.0),
            total_power_now_w: Some(12.4),
            hours_remaining: Some(2.5),
            hours_to_full: None,
        });

        app.push_battery_history();

        assert_eq!(app.charge_history_percent, vec![63]);
        assert_eq!(app.power_draw_history_tenths_w, vec![124]);
    }

    #[test]
    fn refresh_with_cached_tlp_version_avoids_read_only_commands() {
        let runner = FakeCommandRunner::default();

        let snapshot = collect_refresh_snapshot(
            None,
            None,
            Some(TlpVersion::new(1, 9, 0)),
            PrivilegeState {
                effective_root: false,
                sudo_available: false,
            },
        );

        assert_eq!(snapshot.tlp_version, Some(TlpVersion::new(1, 9, 0)));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn applying_refresh_snapshot_tracks_usage_history() {
        let mut app = App::new(
            FakeCommandRunner::default(),
            PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            test_discovery(),
        );

        app.apply_refresh_snapshot(RefreshSnapshot {
            discovery: test_discovery(),
            privilege: PrivilegeState {
                effective_root: true,
                sudo_available: true,
            },
            battery: None,
            usage: Some(jtop_core::UsageSummary {
                cpu_percent: Some(47.6),
                ram: Some(jtop_core::RamUsage {
                    total_bytes: 16 * 1024 * 1024 * 1024,
                    available_bytes: Some(7 * 1024 * 1024 * 1024),
                    used_bytes: 9 * 1024 * 1024 * 1024,
                    used_percent: 56.2,
                }),
                gpus: vec![jtop_core::GpuUsage {
                    name: "GPU0".into(),
                    utilization_percent: Some(72.4),
                    memory_used_bytes: Some(2 * 1024 * 1024 * 1024),
                    memory_total_bytes: Some(8 * 1024 * 1024 * 1024),
                }],
            }),
            previous_cpu_times: None,
            tlp_version: None,
            tlp_status: None,
            tlp_config: None,
            powertop_version: None,
            tlp_config_scan: None,
            tlp_config_preview: None,
            selected_config_target: None,
            logs: Vec::new(),
        });

        assert_eq!(app.cpu_usage_history_percent, vec![48]);
        assert_eq!(app.ram_usage_history_percent, vec![56]);
        assert_eq!(app.gpu_usage_history_percent, vec![72]);
    }
}
