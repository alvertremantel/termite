use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use color_eyre::{Result, eyre::WrapErr};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jones_event::{AppEvent, EventHandler, is_nav_down, is_nav_up, is_quit};
use jones_git_diff::{
    ChangedFile, DetailMode, GitSnapshot, RepoRoot, SnapshotOptions, StdGitRunner, discover_repo,
    load_snapshot,
};
use jones_state::{CoreState, Focus};
use ratatui::{Terminal, backend::Backend};

use crate::ui;

const POLL_INTERVAL: Duration = Duration::from_millis(750);
const EVENT_TICK: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Light,
    Heavy,
}

impl ViewMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Heavy,
            Self::Heavy => Self::Light,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Heavy => "heavy",
        }
    }

    fn detail_mode(self) -> DetailMode {
        DetailMode::FullDiff
    }
}

pub struct JdiffApp {
    pub core: CoreState<()>,
    pub root: RepoRoot,
    pub launched_from: PathBuf,
    pub snapshot: Option<GitSnapshot>,
    pub selected_file: usize,
    pub diff_scroll: u16,
    pub mode: ViewMode,
    pub include_staged: bool,
    pub include_unstaged: bool,
    pub include_untracked: bool,
    pub last_refresh: Option<Instant>,
    pub status: String,
    pub last_error: Option<String>,
    pub(crate) runner: StdGitRunner,
}

impl JdiffApp {
    pub fn new(start: Option<PathBuf>) -> Result<Self> {
        let runner = StdGitRunner;
        let start = start
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)
            .wrap_err("failed to resolve launch directory")?;
        let repo = discover_repo(&runner, &start).wrap_err("failed to discover git repository")?;

        let mut app = Self {
            core: CoreState::new(()),
            root: repo,
            launched_from: start,
            snapshot: None,
            selected_file: 0,
            diff_scroll: 0,
            mode: ViewMode::Light,
            include_staged: true,
            include_unstaged: true,
            include_untracked: true,
            last_refresh: None,
            status: String::from("loading"),
            last_error: None,
            runner,
        };
        app.refresh();
        Ok(app)
    }

    pub async fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        let (mut events, _event_tx) = EventHandler::<()>::new(EVENT_TICK);

        while self.core.running {
            terminal.draw(|frame| ui::render(frame, self))?;

            if let Some(event) = events.next().await {
                self.handle_event(event);
            }
        }

        Ok(())
    }

    pub fn handle_event(&mut self, event: AppEvent<()>) {
        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::Tick => self.refresh_if_due(),
            AppEvent::Resize(_, _) | AppEvent::Mouse(_) | AppEvent::Custom(()) => {}
        }
    }

    pub fn refresh(&mut self) {
        self.last_refresh = Some(Instant::now());
        let previous_path = self.selected_file().map(|file| file.path.clone());
        match load_snapshot(&self.runner, &self.root, self.snapshot_options()) {
            Ok(snapshot) => {
                let visible_files = self.visible_files_for(&snapshot);
                let next_selection = previous_path
                    .as_ref()
                    .and_then(|path| {
                        visible_files
                            .iter()
                            .position(|file| file.path.as_path() == path.as_path())
                    })
                    .unwrap_or_else(|| {
                        self.selected_file
                            .min(visible_files.len().saturating_sub(1))
                    });
                let selected_changed = visible_files
                    .get(next_selection)
                    .map(|file| Some(&file.path) != previous_path.as_ref())
                    .unwrap_or(previous_path.is_some());
                self.selected_file = next_selection;
                if selected_changed {
                    self.diff_scroll = 0;
                }
                self.status = if snapshot.files.is_empty() {
                    String::from("clean")
                } else {
                    format!("{} changed file(s)", visible_files.len())
                };
                self.snapshot = Some(snapshot);
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.status = String::from("error");
            }
        }
    }

    pub fn refresh_if_due(&mut self) {
        if self
            .last_refresh
            .is_none_or(|last_refresh| last_refresh.elapsed() >= POLL_INTERVAL)
        {
            self.refresh();
        }
    }

    pub fn file_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map(|snapshot| self.visible_file_count_for(snapshot))
            .unwrap_or(0)
    }

    pub fn selected_file(&self) -> Option<&ChangedFile> {
        self.visible_files()
            .get(self.selected_file)
            .map(|(_, file)| *file)
    }

    pub fn visible_files(&self) -> Vec<(usize, &ChangedFile)> {
        self.snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| self.file_visible(file))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn visible_files_for<'a>(&self, snapshot: &'a GitSnapshot) -> Vec<&'a ChangedFile> {
        snapshot
            .files
            .iter()
            .filter(|file| self.file_visible(file))
            .collect()
    }

    fn visible_file_count_for(&self, snapshot: &GitSnapshot) -> usize {
        snapshot
            .files
            .iter()
            .filter(|file| self.file_visible(file))
            .count()
    }

    fn file_visible(&self, file: &ChangedFile) -> bool {
        (self.include_staged && file.staged.is_some())
            || (self.include_unstaged && file.unstaged.is_some())
            || (self.include_untracked
                && matches!(file.status, jones_git_diff::FileStatus::Untracked))
    }

    fn snapshot_options(&self) -> SnapshotOptions {
        SnapshotOptions {
            detail: self.mode.detail_mode(),
            include_untracked: self.include_untracked,
            diff_context: 3,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if is_quit(&key) {
            self.core.running = false;
            return;
        }

        if self.core.help_visible {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.core.help_visible = false,
                _ => {}
            }
            return;
        }

        if is_nav_up(&key) {
            self.move_selection_up();
            return;
        }
        if is_nav_down(&key) {
            self.move_selection_down();
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Tab, _) => self.toggle_focus(),
            (KeyCode::BackTab, _) => self.toggle_focus(),
            (KeyCode::Left | KeyCode::Char('h'), _) => self.core.focus = Focus::Sidebar,
            (KeyCode::Right | KeyCode::Char('l'), _) => self.core.focus = Focus::Content,
            (KeyCode::PageUp, _) => self.page_up(),
            (KeyCode::PageDown, _) => self.page_down(),
            (KeyCode::Home | KeyCode::Char('g'), _) => self.jump_top(),
            (KeyCode::End, _) => self.jump_bottom(),
            (KeyCode::Char('G'), _) => self.jump_bottom(),
            (KeyCode::Char('r'), _) => self.refresh(),
            (KeyCode::Char('m'), _) => {
                self.mode = self.mode.toggle();
                self.refresh();
            }
            (KeyCode::Char('s'), _) => {
                self.include_staged = !self.include_staged;
                self.refresh();
            }
            (KeyCode::Char('u'), _) => {
                self.include_unstaged = !self.include_unstaged;
                self.refresh();
            }
            (KeyCode::Char('t'), _) => {
                self.include_untracked = !self.include_untracked;
                self.refresh();
            }
            (KeyCode::Char('?'), _) => self.core.help_visible = true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.core.running = false,
            _ => {}
        }
    }

    fn toggle_focus(&mut self) {
        self.core.focus = match self.core.focus {
            Focus::Sidebar => Focus::Content,
            Focus::Content => Focus::Sidebar,
        };
    }

    fn move_selection_up(&mut self) {
        match self.core.focus {
            Focus::Sidebar if self.selected_file > 0 => {
                self.selected_file -= 1;
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = self.diff_scroll.saturating_sub(1),
            _ => {}
        }
    }

    fn move_selection_down(&mut self) {
        match self.core.focus {
            Focus::Sidebar if self.selected_file + 1 < self.file_count() => {
                self.selected_file += 1;
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = self.diff_scroll.saturating_add(1),
            _ => {}
        }
    }

    fn page_up(&mut self) {
        match self.core.focus {
            Focus::Sidebar => {
                self.selected_file = self.selected_file.saturating_sub(10);
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = self.diff_scroll.saturating_sub(10),
        }
    }

    fn page_down(&mut self) {
        match self.core.focus {
            Focus::Sidebar => {
                self.selected_file =
                    (self.selected_file + 10).min(self.file_count().saturating_sub(1));
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = self.diff_scroll.saturating_add(10),
        }
    }

    fn jump_top(&mut self) {
        match self.core.focus {
            Focus::Sidebar => {
                self.selected_file = 0;
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = 0,
        }
    }

    fn jump_bottom(&mut self) {
        match self.core.focus {
            Focus::Sidebar => {
                self.selected_file = self.file_count().saturating_sub(1);
                self.diff_scroll = 0;
            }
            Focus::Content => self.diff_scroll = u16::MAX / 2,
        }
    }
}
