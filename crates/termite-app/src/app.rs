use color_eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use jones_event as event;
use jones_event::{AppEvent, EventHandler};
use jones_outline::{self as outline, OutlineEntry};
use jones_project_search::{self as project_search, SearchResult};
use jones_search as search;
use jones_search::SearchAction;
use jones_state::{CoreState, Focus};
use jones_syntax::Highlighter;
use jones_text as text;
use jones_workspace::{self as workspace, WorkspaceEntry, WorkspaceOptions};
use ratatui::Terminal;
use ratatui::backend::Backend;
use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use termite_config::Config;
use termite_editor::{ContentMode, EditorAction, EditorContext};

// ---------------------------------------------------------------------------
// External-modification timestamp tracking
// ---------------------------------------------------------------------------

/// Snapshot used to detect external file modifications.
#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    modified: std::time::SystemTime,
}

fn terminal_cwd_osc7_sequence(path: &Path) -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
    format!("\x1b]7;file://{}{}\x07", host, percent_encode_path(path))
}

fn percent_encode_path(path: &Path) -> String {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if is_windows_drive_path(&text) {
        text.insert(0, '/');
    }
    let mut encoded = String::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    for byte in text.as_bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(*byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~');
        if keep {
            encoded.push(*byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }

    encoded
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

pub struct TermiteApp {
    pub core: CoreState<Config>,
    pub file_content: String,
    pub file_scroll: u16,
    pub content_mode: ContentMode,
    pub editor: Option<EditorContext>,
    pub current_file_path: Option<PathBuf>,
    pub highlighter: Highlighter,
    /// `(message, timestamp, is_error)`
    pub notification: Option<(String, Instant, bool)>,
    /// True when the file on disk has changed while in Edit/Split mode.
    pub file_modified_externally: bool,

    // Read-mode in-content search
    pub read_search_active: bool,
    pub read_search_query: String,
    pub read_search_matches: Vec<usize>,
    pub read_search_index: usize,
    pub outline_entries: Vec<OutlineEntry>,
    pub outline_active: bool,
    pub outline_filter: String,
    pub outline_selection: usize,
    pub line_jump_active: bool,
    pub line_jump_buffer: String,
    pub project_search_active: bool,
    pub project_search_query: String,
    pub project_search_results: Vec<SearchResult>,
    pub project_search_selection: usize,
    back_stack: Vec<(PathBuf, usize)>,
    forward_stack: Vec<(PathBuf, usize)>,

    // Mouse drag selection tracking
    /// True while the left mouse button is held after a down event in the editor.
    pub mouse_drag_active: bool,
    /// Timestamp of the last left-button click (for double-click detection).
    pub last_click: Option<Instant>,
    /// Buffer position of the last left-button click (line, col) for double-click detection.
    last_click_pos: Option<(usize, usize)>,

    // ── Workspace state ──────────────────────────────────────────────
    /// The current working directory this workspace is rooted at.
    pub cwd: PathBuf,
    /// Browsable entries in the current working directory.
    pub workspace_entries: Vec<WorkspaceEntry>,
    pub workspace_summary: workspace::WorkspaceSummary,
    pub workspace_options: WorkspaceOptions,
    pub workspace_filter_active: bool,
    /// Full-terminal directory browser mode. Only this mode may change `cwd`.
    pub workspace_fullscreen: bool,
    workspace_fullscreen_previous_focus: Focus,
    pub recent_files: Vec<PathBuf>,
    pub workspace_preview_cache: HashMap<PathBuf, String>,
    /// Index of the highlighted entry in `workspace_entries`.
    pub workspace_selection: usize,
    /// Scroll offset for the workspace panel when there are many entries.
    pub workspace_scroll: u16,
    /// Number of visible rows in the workspace panel (set during rendering).
    pub workspace_viewport_rows: usize,

    /// When `true`, the user is typing a target directory path to change cwd.
    pub cwd_input_active: bool,
    /// The current text in the direct-cwd-input prompt.
    pub cwd_input_buffer: String,

    /// Snapshot of the current file's modification time for external-change detection.
    file_snapshot: Option<FileSnapshot>,

    // Rendering caches
    /// Cached word count to avoid recomputing on every frame.
    pub cached_word_count: usize,
    /// Cached rendered markdown text for read mode.
    pub cached_rendered: Option<ratatui::text::Text<'static>>,
    /// Length of `file_content` when `cached_rendered` was last computed.
    pub cached_render_content_len: usize,
    /// Whether the terminal needs to redraw on the next loop iteration.
    needs_redraw: bool,
}

// ---------------------------------------------------------------------------
// Constructor and helpers
// ---------------------------------------------------------------------------

impl TermiteApp {
    pub fn new(maybe_path: Option<PathBuf>) -> Result<Self> {
        let config = Config::load()?;

        // Resolve the initial cwd and optional file to open.
        let (cwd, open_file) = match maybe_path {
            None => {
                // No args — workspace rooted at the current directory.
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                (cwd, None)
            }
            Some(path) if path.is_dir() => {
                // Argument is a directory — workspace rooted there.
                let cwd = path.canonicalize().unwrap_or(path);
                (cwd, None)
            }
            Some(path) => {
                // Argument is a file — workspace rooted at its parent.
                let path = absolute_path(&path);
                let cwd = path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."));
                let cwd = cwd.canonicalize().unwrap_or(cwd);
                (cwd, Some(path))
            }
        };

        let options = WorkspaceOptions::default();
        let recents = Vec::new();
        let (entries, summary) = workspace::list_workspace_entries(&cwd, &options, &recents);

        // Auto-pick a default file for directory launches when none was specified.
        let file_to_open = open_file.or_else(|| Self::pick_default_file(&cwd));

        let (current_file_path, file_content, file_snapshot) = if let Some(ref fp) = file_to_open {
            let content = if fp.exists() {
                std::fs::read_to_string(fp).unwrap_or_default()
            } else {
                // File doesn't exist yet — create it empty.
                if let Some(parent) = fp.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(fp, "").ok();
                String::new()
            };
            let snap = fp.metadata().ok().map(|m| FileSnapshot {
                path: fp.clone(),
                modified: m.modified().unwrap_or(std::time::UNIX_EPOCH),
            });
            (Some(fp.clone()), content, snap)
        } else {
            (None, String::new(), None)
        };

        let is_markdown = current_file_path.as_ref().is_some_and(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        });

        let content_mode = if is_markdown {
            ContentMode::Read
        } else if current_file_path.is_some() {
            ContentMode::Edit
        } else {
            // No file open — stay in read mode showing welcome screen.
            ContentMode::Read
        };

        let editor = if content_mode == ContentMode::Edit {
            Some(EditorContext::from_content(&file_content))
        } else {
            None
        };

        let highlighter = Highlighter::for_path(current_file_path.as_deref());
        let cached_word_count = file_content.split_whitespace().count();

        let mut core = CoreState::new(config);
        core.focus = Focus::Content;

        let outline_entries = outline::extract_outline(current_file_path.as_deref(), &file_content);

        Ok(Self {
            core,
            file_content,
            file_scroll: 0,
            content_mode,
            editor,
            current_file_path,
            highlighter,
            notification: None,
            file_modified_externally: false,
            read_search_active: false,
            read_search_query: String::new(),
            read_search_matches: Vec::new(),
            read_search_index: 0,
            outline_entries,
            outline_active: false,
            outline_filter: String::new(),
            outline_selection: 0,
            line_jump_active: false,
            line_jump_buffer: String::new(),
            project_search_active: false,
            project_search_query: String::new(),
            project_search_results: Vec::new(),
            project_search_selection: 0,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            mouse_drag_active: false,
            last_click: None,
            last_click_pos: None,
            cwd,
            workspace_entries: entries,
            workspace_summary: summary,
            workspace_options: options,
            workspace_filter_active: false,
            workspace_fullscreen: false,
            workspace_fullscreen_previous_focus: Focus::Content,
            recent_files: recents,
            workspace_preview_cache: HashMap::new(),
            workspace_selection: 0,
            workspace_scroll: 0,
            workspace_viewport_rows: 6,
            cwd_input_active: false,
            cwd_input_buffer: String::new(),
            file_snapshot,
            cached_word_count,
            cached_rendered: None,
            cached_render_content_len: usize::MAX,
            needs_redraw: true,
        })
    }

    // ── Workspace listing ────────────────────────────────────────────

    /// Pick a sensible default file from a directory: first markdown file,
    /// then first regular file.
    fn pick_default_file(cwd: &Path) -> Option<PathBuf> {
        let Ok(read_dir) = std::fs::read_dir(cwd) else {
            return None;
        };

        let mut first_file: Option<PathBuf> = None;

        for entry in read_dir.flatten() {
            let ft = entry.file_type().ok();
            if !ft.is_some_and(|f| f.is_file()) {
                continue;
            }
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            if first_file.is_none() {
                first_file = Some(path.clone());
            }
            // Prefer README or index markdown files.
            if name == "readme.md" || name == "index.md" || name == "readme" {
                return Some(path);
            }
            if name.ends_with(".md") {
                return Some(path);
            }
        }

        first_file
    }

    /// Refresh the workspace listing from the current cwd.
    fn refresh_workspace(&mut self) {
        self.workspace_preview_cache.clear();
        let (entries, summary) = workspace::list_workspace_entries(
            &self.cwd,
            &self.workspace_options,
            &self.recent_files,
        );
        self.workspace_entries = entries;
        self.workspace_summary = summary;
        self.workspace_selection = 0;
        self.workspace_scroll = 0;
        self.needs_redraw = true;
    }

    pub fn selected_workspace_entry(&self) -> Option<&WorkspaceEntry> {
        self.workspace_entries.get(self.workspace_selection)
    }

    pub fn workspace_preview(&mut self) -> String {
        let Some(entry) = self.selected_workspace_entry().cloned() else {
            return String::new();
        };
        match entry.kind {
            workspace::WorkspaceEntryKind::Parent => "Parent directory".to_string(),
            workspace::WorkspaceEntryKind::Directory => "directory".to_string(),
            workspace::WorkspaceEntryKind::File => {
                const MAX_PREVIEW_BYTES: u64 = 16 * 1024;
                if entry.size.is_some_and(|size| size > MAX_PREVIEW_BYTES) {
                    return "preview skipped: file too large".to_string();
                }
                let path = self.cwd.join(&entry.name);
                let key = path.canonicalize().unwrap_or(path);
                if let Some(cached) = self.workspace_preview_cache.get(&key) {
                    return cached.clone();
                }
                let mut buf = String::new();
                let snippet = std::fs::File::open(&key)
                    .and_then(|file| file.take(MAX_PREVIEW_BYTES).read_to_string(&mut buf))
                    .map(|_| {
                        buf.lines()
                            .take(2)
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ⏎ ")
                    })
                    .unwrap_or_else(|_| "preview unavailable".into());
                let snippet: String = if snippet.is_empty() {
                    "empty file".into()
                } else {
                    snippet.chars().take(160).collect()
                };
                self.workspace_preview_cache.insert(key, snippet.clone());
                snippet
            }
        }
    }

    fn remember_recent_file(&mut self, path: &Path) {
        let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.recent_files.retain(|r| r != &p);
        self.recent_files.insert(0, p);
        self.recent_files.truncate(24);
        self.refresh_workspace();
    }

    // ── File loading and cwd changes ─────────────────────────────────

    /// Try to load a file from the workspace. On success, resets scroll, mode,
    /// and caches.  Returns `false` when the file cannot be read.
    fn load_file(&mut self, path: &Path) -> bool {
        let path = absolute_path(path);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                self.notification = Some((
                    format!("Cannot open {}: {e}", path.display()),
                    Instant::now(),
                    true,
                ));
                return false;
            }
        };

        let is_markdown = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));

        // Drop old editor state completely.
        self.editor = None;
        self.read_search_active = false;
        self.read_search_query.clear();
        self.read_search_matches.clear();
        self.read_search_index = 0;
        self.outline_active = false;
        self.line_jump_active = false;
        self.project_search_active = false;
        self.file_modified_externally = false;

        self.file_content = content;
        self.file_scroll = 0;
        self.current_file_path = Some(path.clone());
        self.highlighter = Highlighter::for_path(Some(&path));
        self.outline_entries = outline::extract_outline(Some(&path), &self.file_content);

        self.file_snapshot = path.metadata().ok().map(|m| FileSnapshot {
            path: path.clone(),
            modified: m.modified().unwrap_or(std::time::UNIX_EPOCH),
        });

        if is_markdown {
            self.content_mode = ContentMode::Read;
        } else {
            self.content_mode = ContentMode::Edit;
            self.editor = Some(EditorContext::from_content(&self.file_content));
        }

        self.invalidate_content_caches();
        self.notification = Some((
            format!(
                "Opened {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Instant::now(),
            false,
        ));
        self.remember_recent_file(&path);
        true
    }

    /// Change the workspace cwd and refresh the listing. If configured, also
    /// updates Termite's process cwd and emits a terminal OSC 7 cwd hint.
    pub fn change_cwd(&mut self, path: PathBuf) {
        let canonical = path.canonicalize().unwrap_or(path);
        if !canonical.is_dir() {
            self.notification = Some((
                format!("Not a directory: {}", canonical.display()),
                Instant::now(),
                true,
            ));
            return;
        }
        self.cwd = canonical;
        let sync_error = self.sync_terminal_cwd_if_enabled();
        self.refresh_workspace();

        // If no file is open, auto-pick a default from the new cwd.
        let auto_opened = if self.current_file_path.is_none() {
            if let Some(default) = Self::pick_default_file(&self.cwd) {
                self.load_file(&default);
                true
            } else {
                false
            }
        } else {
            false
        };

        // Only show a "Workspace:" notification if we didn't auto-open a file
        // (load_file already sets an "Opened ..." notification).
        if !auto_opened {
            let sync_failed = sync_error.is_some();
            self.notification = Some((
                if let Some(err) = &sync_error {
                    format!("Workspace: {} (cwd sync failed: {err})", self.cwd.display())
                } else {
                    format!("Workspace: {}", self.cwd.display())
                },
                Instant::now(),
                sync_failed,
            ));
        }
    }

    fn sync_terminal_cwd_if_enabled(&self) -> Option<String> {
        if !self.core.config.workspace.sync_terminal_cwd {
            return None;
        }

        if let Err(err) = std::env::set_current_dir(&self.cwd) {
            return Some(err.to_string());
        }

        let mut stdout = std::io::stdout();
        if stdout.is_terminal() {
            let osc7 = terminal_cwd_osc7_sequence(&self.cwd);
            if let Err(err) = stdout
                .write_all(osc7.as_bytes())
                .and_then(|_| stdout.flush())
            {
                return Some(err.to_string());
            }
        }

        None
    }

    /// Navigate to the parent directory.
    fn navigate_to_parent(&mut self) {
        if !self.workspace_fullscreen {
            self.prompt_for_fullscreen_directory_browser("Use F2 to change folders");
            return;
        }
        if let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf()) {
            self.change_cwd(parent);
        }
    }

    fn open_workspace_browser(&mut self) {
        if !self.workspace_fullscreen {
            self.workspace_fullscreen_previous_focus = self.core.focus;
        }
        self.workspace_fullscreen = true;
        self.core.focus = Focus::Sidebar;
        self.workspace_filter_active = false;
        self.notification = Some((
            "Directory browser: Enter changes folders, Esc returns".to_string(),
            Instant::now(),
            false,
        ));
    }

    fn close_workspace_browser(&mut self) {
        self.workspace_fullscreen = false;
        self.workspace_filter_active = false;
        self.core.focus = if self.workspace_fullscreen_previous_focus == Focus::Sidebar
            && self.core.sidebar_visible
        {
            Focus::Sidebar
        } else {
            Focus::Content
        };
    }

    fn toggle_sidebar_visibility(&mut self) {
        self.core.sidebar_visible = !self.core.sidebar_visible;
        if !self.core.sidebar_visible && self.core.focus == Focus::Sidebar {
            self.core.focus = Focus::Content;
        }
        self.notification = Some((
            if self.core.sidebar_visible {
                "Sidebar shown (Tab focuses it)".to_string()
            } else {
                "Sidebar hidden (Ctrl+B shows it)".to_string()
            },
            Instant::now(),
            false,
        ));
    }

    fn prompt_for_fullscreen_directory_browser(&mut self, message: &str) {
        if !self.workspace_fullscreen {
            self.workspace_fullscreen_previous_focus = self.core.focus;
        }
        self.workspace_fullscreen = true;
        self.core.focus = Focus::Sidebar;
        self.notification = Some((
            format!("{message}; press Enter again"),
            Instant::now(),
            false,
        ));
    }

    /// Try to save the current editor buffer before leaving it.
    /// Returns `true` when it is safe to proceed (saved or not dirty).
    fn try_save_before_switch(&mut self) -> bool {
        if self.editor.as_ref().is_some_and(|e| e.is_dirty()) {
            self.save_editor_file();
            // After saving, check if the buffer is still dirty (save failed).
            if self.editor.as_ref().is_some_and(|e| e.is_dirty()) {
                return false;
            }
        }
        true
    }

    /// Open the currently highlighted workspace entry.
    fn open_workspace_entry(&mut self) {
        if self.workspace_entries.is_empty() {
            return;
        }
        let entry = self.workspace_entries[self.workspace_selection].clone();
        match entry.kind {
            workspace::WorkspaceEntryKind::Parent => {
                if self.workspace_fullscreen {
                    self.navigate_to_parent();
                } else {
                    self.prompt_for_fullscreen_directory_browser("Use F2 to change folders");
                }
            }
            workspace::WorkspaceEntryKind::Directory => {
                if self.workspace_fullscreen {
                    let new_cwd = self.cwd.join(&entry.name);
                    self.change_cwd(new_cwd);
                } else {
                    self.prompt_for_fullscreen_directory_browser(
                        "Directory changes happen in F2 browser",
                    );
                }
            }
            workspace::WorkspaceEntryKind::File => {
                // Guard: save dirty buffer before switching.
                if !self.try_save_before_switch() {
                    return; // Save failed, abort.
                }
                let file_path = self.cwd.join(&entry.name);
                self.load_file(&file_path);
                self.workspace_fullscreen = false;
                self.core.focus = Focus::Content;
            }
        }
    }

    // ── Caches ───────────────────────────────────────────────────────

    /// Update caches after `file_content` has been changed.
    fn invalidate_content_caches(&mut self) {
        self.cached_word_count = self.file_content.split_whitespace().count();
        self.cached_rendered = None;
        self.cached_render_content_len = usize::MAX;
        self.outline_entries =
            outline::extract_outline(self.current_file_path.as_deref(), &self.file_content);
    }

    // ── Event loop ───────────────────────────────────────────────────

    pub async fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        let (mut events, _event_tx) = EventHandler::<()>::new(Duration::from_millis(100));

        while self.core.running {
            // Check for external file modifications on each loop iteration.
            self.poll_file_modified();

            if self.needs_redraw {
                terminal.draw(|frame| crate::draw::draw(frame, self))?;
                self.needs_redraw = false;
            }
            if let Some(ev) = events.next().await {
                self.handle_event(ev);
            }
        }
        Ok(())
    }

    /// Detect external modifications by comparing the current mtime snapshot.
    fn poll_file_modified(&mut self) {
        let Some(ref snap) = self.file_snapshot else {
            return;
        };
        let Ok(meta) = std::fs::metadata(&snap.path) else {
            return;
        };
        let Ok(mtime) = meta.modified() else {
            return;
        };
        if mtime != snap.modified
            && matches!(self.content_mode, ContentMode::Edit | ContentMode::Split)
        {
            self.file_modified_externally = true;
        }
    }

    fn handle_event(&mut self, ev: AppEvent<()>) {
        match ev {
            AppEvent::Key(key) => {
                self.needs_redraw = true;

                // ── Help overlay ─────────────────────────────────────
                if self.core.help_visible {
                    if key.code == KeyCode::Char('?') || key.code == KeyCode::Esc {
                        self.core.help_visible = false;
                    }
                    return;
                }

                // ── Direct cwd input mode ────────────────────────────
                if self.cwd_input_active {
                    self.handle_cwd_input_key(key);
                    return;
                }
                if self.outline_active {
                    self.handle_outline_key(key);
                    return;
                }
                if self.line_jump_active {
                    self.handle_line_jump_key(key);
                    return;
                }
                if self.project_search_active {
                    self.handle_project_search_key(key);
                    return;
                }

                if self.workspace_filter_active {
                    self.handle_workspace_filter_key(key);
                    return;
                }

                // ── Search mode ──────────────────────────────────────
                if self.core.searching {
                    self.handle_search_key(key);
                    return;
                }

                // ── Read-mode in-content search ──────────────────────
                if self.read_search_active {
                    self.handle_read_search_key(key);
                    return;
                }

                // ── Global workspace visibility keys ─────────────────
                if key.code == KeyCode::F(2) {
                    if self.workspace_fullscreen {
                        self.close_workspace_browser();
                    } else {
                        self.open_workspace_browser();
                    }
                    return;
                }

                if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.toggle_sidebar_visibility();
                    return;
                }

                // ── Global: Ctrl+E = toggle focus (always available) ─
                if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if self.workspace_fullscreen {
                        self.close_workspace_browser();
                        return;
                    }
                    if !self.core.sidebar_visible {
                        self.notification = Some((
                            "Sidebar is hidden; Ctrl+B shows it".to_string(),
                            Instant::now(),
                            false,
                        ));
                        self.core.focus = Focus::Content;
                        return;
                    }
                    self.core.focus = match self.core.focus {
                        Focus::Sidebar => Focus::Content,
                        Focus::Content => Focus::Sidebar,
                    };
                    self.notification = Some((
                        match self.core.focus {
                            Focus::Sidebar => "Workspace focus".to_string(),
                            Focus::Content => "Content focus".to_string(),
                        },
                        Instant::now(),
                        false,
                    ));
                    return;
                }

                // ── Global keys: Tab = toggle focus (except in edit/split where Tab indents) ─
                if self.workspace_fullscreen
                    && (key.code == KeyCode::Tab || key.code == KeyCode::BackTab)
                {
                    self.close_workspace_browser();
                    return;
                }

                let in_editing =
                    matches!(self.content_mode, ContentMode::Edit | ContentMode::Split);
                if key.code == KeyCode::Tab
                    && !key.modifiers.contains(KeyModifiers::SHIFT)
                    && !in_editing
                {
                    if self.workspace_fullscreen {
                        self.close_workspace_browser();
                    } else if self.core.sidebar_visible {
                        self.core.focus = match self.core.focus {
                            Focus::Sidebar => Focus::Content,
                            Focus::Content => Focus::Sidebar,
                        };
                    } else {
                        self.notification = Some((
                            "Sidebar is hidden; Ctrl+B shows it".to_string(),
                            Instant::now(),
                            false,
                        ));
                    }
                    return;
                }
                if key.code == KeyCode::BackTab && !in_editing {
                    if self.workspace_fullscreen {
                        self.close_workspace_browser();
                    } else if self.core.sidebar_visible {
                        self.core.focus = match self.core.focus {
                            Focus::Sidebar => Focus::Content,
                            Focus::Content => Focus::Sidebar,
                        };
                    }
                    return;
                }

                // ── Sidebar-focused workspace keys ─────────────────
                if self.workspace_fullscreen || self.core.focus == Focus::Sidebar {
                    self.handle_workspace_key(key);
                    return;
                }

                if self.content_mode == ContentMode::Read {
                    match key.code {
                        KeyCode::Char('o') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.open_outline();
                            return;
                        }
                        KeyCode::Char(':') => {
                            self.line_jump_active = true;
                            self.line_jump_buffer.clear();
                            return;
                        }
                        KeyCode::Char('r') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.project_search_active = true;
                            self.project_search_query.clear();
                            self.project_search_results.clear();
                            self.project_search_selection = 0;
                            return;
                        }
                        KeyCode::Char('[') => {
                            self.go_back();
                            return;
                        }
                        KeyCode::Char(']') => {
                            self.go_forward();
                            return;
                        }
                        _ => {}
                    }
                }

                // ── Content-focused keys ────────────────────────────
                // Handle editor mode (Edit or Split both use the editor)
                if matches!(self.content_mode, ContentMode::Edit | ContentMode::Split) {
                    self.handle_editor_key(key);
                    return;
                }

                // Normal read mode (markdown or welcome screen)
                if event::is_quit(&key) {
                    self.core.running = false;
                    return;
                }
                match key.code {
                    KeyCode::Char('/') => {
                        self.core.searching = true;
                        self.core.search_query.clear();
                        self.core.search_results.clear();
                        self.core.search_index = 0;
                    }
                    KeyCode::Char('?') => {
                        self.core.help_visible = true;
                    }
                    KeyCode::Char('e') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.enter_edit_mode();
                    }
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.reset_read_search();
                        self.read_search_active = true;
                    }
                    _ => {
                        // In welcome-screen read mode, only navigate keys apply.
                        if self.current_file_path.is_some() {
                            self.handle_read_key(key);
                        }
                    }
                }
            }
            AppEvent::Mouse(mouse) => {
                self.needs_redraw = true;
                self.handle_mouse(mouse);
            }
            AppEvent::Tick => {
                let now = Instant::now();
                // Fade notifications after 3 seconds
                if let Some((_, t, _)) = &self.notification
                    && now.duration_since(*t) > Duration::from_secs(3)
                {
                    self.notification = None;
                    self.needs_redraw = true;
                }
                // Redraw on tick only if a notification is visible (it may be fading)
                if self.notification.is_some() {
                    self.needs_redraw = true;
                }
            }
            AppEvent::Resize(_, _) => {
                self.needs_redraw = true;
            }
            AppEvent::Custom(()) => {}
        }
    }

    fn current_line(&self) -> usize {
        match self.content_mode {
            ContentMode::Edit | ContentMode::Split => self
                .editor
                .as_ref()
                .map(|e| e.state.cursor_line)
                .unwrap_or(0),
            ContentMode::Read => self.file_scroll as usize,
        }
    }
    fn push_history(&mut self) {
        if let Some(p) = self.current_file_path.clone() {
            self.back_stack.push((p, self.current_line()));
            self.forward_stack.clear();
        }
    }
    fn jump_to_line(&mut self, line: usize) {
        self.push_history();
        self.set_position(line);
    }
    fn set_position(&mut self, line: usize) {
        match self.content_mode {
            ContentMode::Edit | ContentMode::Split => {
                if let Some(e) = &mut self.editor {
                    let max = e.buffer.line_count().saturating_sub(1);
                    e.state.cursor_line = line.min(max);
                    e.state.cursor_col = 0;
                    e.state.scroll_offset = e.state.cursor_line;
                }
            }
            ContentMode::Read => {
                self.file_scroll = Self::line_to_scroll(line).min(self.max_read_scroll())
            }
        }
    }
    fn open_outline(&mut self) {
        self.outline_entries =
            outline::extract_outline(self.current_file_path.as_deref(), &self.file_content);
        self.outline_active = true;
        self.outline_filter.clear();
        self.outline_selection = 0;
    }
    pub fn filtered_outline_indices(&self) -> Vec<usize> {
        let q = self.outline_filter.to_lowercase();
        self.outline_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| q.is_empty() || e.label.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }
    pub fn current_breadcrumb(&self) -> Option<String> {
        outline::breadcrumb(&self.outline_entries, self.current_line())
    }
    fn open_path_at_line(&mut self, path: PathBuf, line: usize) {
        self.push_history();
        if self.current_file_path.as_ref() != Some(&path) {
            if !self.try_save_before_switch() {
                return;
            }
            self.load_file(&path);
        }
        self.set_position(line);
    }
    fn go_back(&mut self) {
        if let Some(loc) = self.back_stack.pop() {
            if let Some(p) = self.current_file_path.clone() {
                self.forward_stack.push((p, self.current_line()));
            }
            self.open_path_without_history(loc.0, loc.1);
        }
    }
    fn go_forward(&mut self) {
        if let Some(loc) = self.forward_stack.pop() {
            if let Some(p) = self.current_file_path.clone() {
                self.back_stack.push((p, self.current_line()));
            }
            self.open_path_without_history(loc.0, loc.1);
        }
    }
    fn open_path_without_history(&mut self, path: PathBuf, line: usize) {
        if self.current_file_path.as_ref() != Some(&path) {
            if !self.try_save_before_switch() {
                return;
            }
            if !self.load_file(&path) {
                return;
            }
        }
        self.set_position(line);
    }

    fn handle_outline_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => self.outline_active = false,
            KeyCode::Backspace => {
                self.outline_filter.pop();
                self.outline_selection = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let n = self.filtered_outline_indices().len();
                if n > 0 {
                    self.outline_selection = (self.outline_selection + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.outline_selection = self.outline_selection.saturating_sub(1)
            }
            KeyCode::Enter => {
                let idxs = self.filtered_outline_indices();
                if let Some(idx) = idxs.get(self.outline_selection) {
                    let line = self.outline_entries[*idx].line;
                    self.outline_active = false;
                    self.jump_to_line(line);
                }
            }
            KeyCode::Char(c) => {
                self.outline_filter.push(c);
                self.outline_selection = 0;
            }
            _ => {}
        }
    }
    fn handle_line_jump_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => self.line_jump_active = false,
            KeyCode::Backspace => {
                self.line_jump_buffer.pop();
            }
            KeyCode::Enter => {
                if let Ok(n) = self.line_jump_buffer.parse::<usize>() {
                    self.jump_to_line(n.saturating_sub(1));
                }
                self.line_jump_active = false;
            }
            KeyCode::Char(c) if c.is_ascii_digit() => self.line_jump_buffer.push(c),
            _ => {}
        }
    }
    fn handle_project_search_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => self.project_search_active = false,
            KeyCode::Backspace => {
                self.project_search_query.pop();
                self.project_search_results =
                    project_search::search_project(&self.cwd, &self.project_search_query, 100);
                self.project_search_selection = 0;
            }
            KeyCode::Down | KeyCode::Char('j') if !self.project_search_results.is_empty() => {
                self.project_search_selection =
                    (self.project_search_selection + 1).min(self.project_search_results.len() - 1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.project_search_selection = self.project_search_selection.saturating_sub(1)
            }
            KeyCode::Enter => {
                if let Some(r) = self
                    .project_search_results
                    .get(self.project_search_selection)
                    .cloned()
                {
                    self.project_search_active = false;
                    self.open_path_at_line(r.path, r.line);
                }
            }
            KeyCode::Char(c) => {
                self.project_search_query.push(c);
                self.project_search_results =
                    project_search::search_project(&self.cwd, &self.project_search_query, 100);
                self.project_search_selection = 0;
            }
            _ => {}
        }
    }

    // ── Workspace (sidebar) keyboard ─────────────────────────────────

    fn handle_workspace_key(&mut self, key: crossterm::event::KeyEvent) {
        if event::is_quit(&key) {
            // Auto-save dirty buffer before quitting.
            if !self.try_save_before_switch() {
                // Save failed — abort quit.
                return;
            }
            self.core.running = false;
            return;
        }

        match key.code {
            KeyCode::Esc if self.workspace_fullscreen => {
                self.close_workspace_browser();
            }
            KeyCode::Char('?') => {
                self.core.help_visible = true;
            }
            KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.workspace_fullscreen {
                    // Enter direct-cwd input mode. Directory changes are only
                    // available from the full-screen browser.
                    self.cwd_input_active = true;
                    self.cwd_input_buffer = self.cwd.display().to_string();
                } else {
                    self.workspace_fullscreen_previous_focus = self.core.focus;
                    self.workspace_fullscreen = true;
                    self.core.focus = Focus::Sidebar;
                    self.cwd_input_active = true;
                    self.cwd_input_buffer = self.cwd.display().to_string();
                }
            }
            KeyCode::Char('/') | KeyCode::Char('f') => {
                self.workspace_filter_active = true;
                self.workspace_options.filter.clear();
                self.refresh_workspace();
            }
            KeyCode::Char('s') => {
                self.workspace_options.sort_mode = self.workspace_options.sort_mode.cycle();
                self.refresh_workspace();
                self.notification = Some((
                    format!(
                        "Workspace sort: {}",
                        self.workspace_options.sort_mode.label()
                    ),
                    Instant::now(),
                    false,
                ));
            }
            KeyCode::Char('.') => {
                self.workspace_options.show_hidden = !self.workspace_options.show_hidden;
                self.refresh_workspace();
            }
            KeyCode::Char('o') => {
                self.workspace_options.scope = self.workspace_options.scope.cycle();
                self.refresh_workspace();
            }
            KeyCode::Enter => {
                self.open_workspace_entry();
            }
            KeyCode::Char('h') | KeyCode::Backspace | KeyCode::Left => {
                // Navigate to parent directory.
                self.navigate_to_parent();
            }
            KeyCode::Char('g') => {
                // Select parent entry (or first entry) and go to top.
                self.workspace_selection = 0;
                self.workspace_scroll = 0;
            }
            KeyCode::Home => {
                self.workspace_selection = 0;
                self.workspace_scroll = 0;
            }
            KeyCode::Char('G') if !self.workspace_entries.is_empty() => {
                // Jump to last entry.
                self.workspace_selection = self.workspace_entries.len() - 1;
            }
            KeyCode::End if !self.workspace_entries.is_empty() => {
                self.workspace_selection = self.workspace_entries.len() - 1;
            }
            KeyCode::Char('j') | KeyCode::Down if !self.workspace_entries.is_empty() => {
                self.workspace_selection =
                    (self.workspace_selection + 1).min(self.workspace_entries.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.workspace_selection = self.workspace_selection.saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char(' ') if !self.workspace_entries.is_empty() => {
                // Page down in workspace list.
                let step = self.workspace_viewport_rows.max(1);
                self.workspace_selection =
                    (self.workspace_selection + step).min(self.workspace_entries.len() - 1);
            }
            KeyCode::PageUp if !self.workspace_entries.is_empty() => {
                let step = self.workspace_viewport_rows.max(1);
                self.workspace_selection = self.workspace_selection.saturating_sub(step);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                // Enter directory or open file (same as Enter).
                self.open_workspace_entry();
            }
            _ => {}
        }

        // Clamp selection after any mutation.
        if !self.workspace_entries.is_empty() {
            self.workspace_selection = self
                .workspace_selection
                .min(self.workspace_entries.len().saturating_sub(1));
        } else {
            self.workspace_selection = 0;
        }

        // Auto-scroll workspace panel so selection stays visible.
        self.scroll_workspace_to_selection();
    }

    fn handle_workspace_filter_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.workspace_filter_active = false;
                self.workspace_options.filter.clear();
                self.refresh_workspace();
            }
            KeyCode::Enter => self.workspace_filter_active = false,
            KeyCode::Backspace => {
                self.workspace_options.filter.pop();
                self.refresh_workspace();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.workspace_options.filter.push(c);
                self.refresh_workspace();
            }
            _ => {}
        }
    }

    /// Adjust `workspace_scroll` so the current selection is visible.
    pub fn scroll_workspace_to_selection(&mut self) {
        let viewport = self.workspace_viewport_rows.max(1);
        let sel = self.workspace_selection;
        let scroll = self.workspace_scroll as usize;
        if sel < scroll {
            self.workspace_scroll = sel as u16;
        } else if sel >= scroll + viewport {
            self.workspace_scroll = (sel.saturating_sub(viewport.saturating_sub(1))) as u16;
        }
    }

    // ── Direct cwd input ─────────────────────────────────────────────

    fn handle_cwd_input_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.cwd_input_active = false;
                self.cwd_input_buffer.clear();
            }
            KeyCode::Enter => {
                let path = PathBuf::from(self.cwd_input_buffer.clone());
                // Resolve relative paths against the workspace cwd.
                let resolved = if path.is_relative() {
                    self.cwd.join(&path)
                } else {
                    path
                };
                self.cwd_input_active = false;
                self.change_cwd(resolved);
                self.cwd_input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.cwd_input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.cwd_input_buffer.push(c);
            }
            _ => {}
        }
    }

    // ── Editor key handling ──────────────────────────────────────────

    fn handle_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        let viewport_h = self.core.content_area.height.saturating_sub(2) as usize;

        // Capture buffer version before the key is handled so we can detect mutations.
        let version_before = self.editor.as_ref().map(|e| e.buffer.version());

        // Get the action from the editor, then drop the borrow so we can
        // call &mut self methods like save_editor_file below.
        let action = match self.editor.as_mut() {
            Some(editor) => {
                editor.viewport_height = viewport_h;
                editor.handle_key(key)
            }
            None => return,
        };

        // If the buffer was mutated, update the cached word count.
        if let Some(editor) = &self.editor
            && version_before != Some(editor.buffer.version())
        {
            self.cached_word_count = editor.buffer.text().split_whitespace().count();
        }

        match action {
            EditorAction::ExitEditor => {
                // Auto-save if dirty
                if self.editor.as_ref().is_some_and(|e| e.is_dirty()) {
                    self.save_editor_file();
                }
                // Sync buffer back to file_content for the preview
                if let Some(editor) = &self.editor {
                    self.file_content = editor.text();
                    self.file_scroll = Self::line_to_scroll(editor.state.scroll_offset);
                }
                self.invalidate_content_caches();
                self.content_mode = ContentMode::Read;
            }
            EditorAction::SaveFile => {
                self.save_editor_file();
            }
            EditorAction::ToggleSplitPreview => {
                self.content_mode = match self.content_mode {
                    ContentMode::Edit => ContentMode::Split,
                    ContentMode::Split => ContentMode::Edit,
                    _ => self.content_mode,
                };
            }
            EditorAction::ReloadFile => {
                if self.file_modified_externally
                    && let Some(path) = &self.current_file_path
                    && let Ok(content) = std::fs::read_to_string(path)
                {
                    self.file_content = content.clone();
                    self.editor = Some(EditorContext::from_content(&content));
                    self.file_modified_externally = false;
                    self.invalidate_content_caches();
                }
            }
            EditorAction::Find => {
                // Find bar is now active inside the editor context;
                // nothing else to do at the app level.
            }
            EditorAction::None => {}
        }
        // Keep cursor visible
        if let Some(editor) = &mut self.editor {
            editor.ensure_visible(viewport_h);
        }
    }

    fn enter_edit_mode(&mut self) {
        // Dismiss read-mode search when entering edit mode
        self.read_search_active = false;
        let mut editor = EditorContext::from_content(&self.file_content);
        // Preserve scroll position from read mode
        editor.state.scroll_offset = self.file_scroll as usize;
        editor.state.cursor_line = self.file_scroll as usize;
        // Clamp cursor_line to valid buffer range
        editor.state.cursor_line = editor
            .state
            .cursor_line
            .min(editor.buffer.line_count().saturating_sub(1));
        self.editor = Some(editor);
        self.content_mode = ContentMode::Edit;
        self.core.focus = Focus::Content;
    }

    /// Save the current editor buffer to disk and set a notification.
    fn save_editor_file(&mut self) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let Some(path) = self.current_file_path.clone() else {
            return;
        };
        match editor.save(&path) {
            Ok(()) => {
                self.file_content = editor.text();
                self.invalidate_content_caches();
                self.file_modified_externally = false;
                // Update snapshot on successful save.
                if let Ok(meta) = std::fs::metadata(&path) {
                    self.file_snapshot = Some(FileSnapshot {
                        path: path.clone(),
                        modified: meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    });
                }
                self.notification = Some((
                    format!(
                        "Saved {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    Instant::now(),
                    false,
                ));
            }
            Err(e) => {
                self.notification = Some((format!("Save failed: {e}"), Instant::now(), true));
            }
        }
    }

    // ── Read-mode keyboard ───────────────────────────────────────────

    fn handle_read_key(&mut self, key: crossterm::event::KeyEvent) {
        let viewport_height = self.core.content_area.height as usize;

        if event::is_nav_up(&key) {
            self.file_scroll = self.file_scroll.saturating_sub(1);
        } else if event::is_nav_down(&key) {
            self.file_scroll = self.file_scroll.saturating_add(1);
        } else if key.code == KeyCode::Char(' ') {
            // Page down
            self.file_scroll = self.file_scroll.saturating_add(viewport_height as u16);
        } else if key.code == KeyCode::Char('b') {
            // Page up (vim Ctrl+B equivalent)
            self.file_scroll = self.file_scroll.saturating_sub(viewport_height as u16);
        } else if key.code == KeyCode::Char('g') {
            self.file_scroll = 0;
        } else if key.code == KeyCode::Char('G') {
            // Jump to bottom of document
            self.file_scroll = Self::line_to_scroll(
                self.file_content
                    .lines()
                    .count()
                    .saturating_sub(viewport_height),
            );
        } else if key.code == KeyCode::Char('d') {
            // Half-page down
            let half = (viewport_height / 2) as u16;
            self.file_scroll = self.file_scroll.saturating_add(half);
        } else if key.code == KeyCode::Char('u') {
            // Half-page up
            let half = (viewport_height / 2) as u16;
            self.file_scroll = self.file_scroll.saturating_sub(half);
        }
        // Clamp scroll to content bounds
        self.file_scroll = self.file_scroll.min(self.max_read_scroll());
    }

    // ── Mouse handling ───────────────────────────────────────────────

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        // Ignore all mouse events while the cwd-input modal is active.
        if self.cwd_input_active {
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Dismiss modal overlays on any click.
                if self.core.help_visible || self.core.searching {
                    self.core.help_visible = false;
                    self.core.searching = false;
                    return;
                }
                let col = mouse.column;
                let row = mouse.row;

                // Check if click is in the workspace panel area.
                if col >= self.core.sidebar_area.x
                    && row >= self.core.sidebar_area.y
                    && row < self.core.sidebar_area.y + self.core.sidebar_area.height
                    && !self.workspace_entries.is_empty()
                {
                    // Click within workspace panel — select entry.
                    self.core.focus = Focus::Sidebar;
                    let rel_row = (row.saturating_sub(self.core.sidebar_area.y + 1)) as usize; // skip header
                    let idx = self.workspace_scroll as usize + rel_row;
                    if idx < self.workspace_entries.len() {
                        self.workspace_selection = idx;
                        self.needs_redraw = true;
                    }
                    return;
                }

                if col >= self.core.content_area.x && row >= self.core.content_area.y {
                    self.core.focus = Focus::Content;
                    // If in edit/split mode, handle click positioning + drag/double-click
                    if matches!(self.content_mode, ContentMode::Edit | ContentMode::Split)
                        && let Some(editor) = &mut self.editor
                        && let Some((line, char_col)) =
                            Self::screen_to_char_pos(editor, self.core.content_area, col, row)
                    {
                        // Double-click detection: select word if same position within 400ms
                        let now = Instant::now();
                        let is_double_click = self
                            .last_click
                            .is_some_and(|t| now.duration_since(t) < Duration::from_millis(400))
                            && self.last_click_pos == Some((line, char_col));

                        if is_double_click {
                            let rope = editor.buffer.rope();
                            editor.state.move_cursor_to(line, char_col, rope);
                            editor.state.select_word(editor.buffer.rope());
                            self.mouse_drag_active = false;
                            // Clear last_click so a third click doesn't re-trigger
                            self.last_click = None;
                            self.last_click_pos = None;
                        } else {
                            // Normal click: position cursor, start potential drag
                            let rope = editor.buffer.rope();
                            editor.state.clear_selection();
                            editor.state.move_cursor_to(line, char_col, rope);
                            editor.state.start_selection();
                            self.mouse_drag_active = true;
                            self.last_click = Some(now);
                            self.last_click_pos = Some((line, char_col));
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click in the workspace panel: go to parent.
                let col = mouse.column;
                let row = mouse.row;
                if self.workspace_fullscreen
                    && col >= self.core.sidebar_area.x
                    && row >= self.core.sidebar_area.y
                    && row < self.core.sidebar_area.y + self.core.sidebar_area.height
                {
                    self.core.focus = Focus::Sidebar;
                    self.navigate_to_parent();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.mouse_drag_active
                    && matches!(self.content_mode, ContentMode::Edit | ContentMode::Split)
                    && let Some(editor) = &mut self.editor
                    && let Some((line, char_col)) = Self::screen_to_char_pos(
                        editor,
                        self.core.content_area,
                        mouse.column,
                        mouse.row,
                    )
                {
                    let rope = editor.buffer.rope();
                    editor.state.move_cursor_to(line, char_col, rope);
                    editor.state.extend_selection();
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.mouse_drag_active => {
                self.mouse_drag_active = false;
                // If selection start == end (no actual drag), clear it
                if let Some(editor) = &mut self.editor
                    && let Some(sel) = &editor.state.selection
                    && sel.anchor_line == sel.head_line
                    && sel.anchor_col == sel.head_col
                {
                    editor.state.clear_selection();
                }
            }
            MouseEventKind::ScrollUp => {
                if matches!(self.content_mode, ContentMode::Edit | ContentMode::Split) {
                    if let Some(editor) = &mut self.editor {
                        editor.state.scroll_offset = editor.state.scroll_offset.saturating_sub(3);
                    }
                } else {
                    self.file_scroll = self.file_scroll.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if matches!(self.content_mode, ContentMode::Edit | ContentMode::Split) {
                    if let Some(editor) = &mut self.editor {
                        editor.state.scroll_offset += 3;
                        let max = editor.buffer.line_count().saturating_sub(1);
                        editor.state.scroll_offset = editor.state.scroll_offset.min(max);
                    }
                } else {
                    self.file_scroll = self.file_scroll.saturating_add(3);
                    self.file_scroll = self.file_scroll.min(self.max_read_scroll());
                }
            }
            _ => {}
        }
    }

    // ── Search handling ──────────────────────────────────────────────

    fn handle_search_key(&mut self, key: crossterm::event::KeyEvent) {
        let action = search::handle_search_key(&mut self.core, key);
        match action {
            SearchAction::Selected(_idx) => {
                // No-op in termite (no file list)
            }
            SearchAction::Updated => {
                // No-op in termite (no file list)
            }
            _ => {}
        }
    }

    /// Handle key events while the read-mode in-content search bar is active.
    fn handle_read_search_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.read_search_active = false;
            }
            KeyCode::Backspace => {
                self.read_search_query.pop();
                self.update_read_search_matches();
            }
            KeyCode::Enter if !self.read_search_matches.is_empty() => {
                // Jump to current match, then advance index for subsequent presses
                let line = self.read_search_matches[self.read_search_index];
                self.file_scroll = Self::line_to_scroll(line);
                self.read_search_index =
                    (self.read_search_index + 1) % self.read_search_matches.len();
            }
            KeyCode::Char('n')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !self.read_search_matches.is_empty() =>
            {
                // Ctrl+N: next match (alternative to Enter)
                self.read_search_index =
                    (self.read_search_index + 1) % self.read_search_matches.len();
                let line = self.read_search_matches[self.read_search_index];
                self.file_scroll = Self::line_to_scroll(line);
            }
            KeyCode::Char('p')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !self.read_search_matches.is_empty() =>
            {
                // Ctrl+P: previous match
                if self.read_search_index == 0 {
                    self.read_search_index = self.read_search_matches.len() - 1;
                } else {
                    self.read_search_index -= 1;
                }
                let line = self.read_search_matches[self.read_search_index];
                self.file_scroll = Self::line_to_scroll(line);
            }
            KeyCode::Char(c) => {
                self.read_search_query.push(c);
                self.update_read_search_matches();
            }
            _ => {}
        }
    }

    fn reset_read_search(&mut self) {
        self.read_search_query.clear();
        self.read_search_matches.clear();
        self.read_search_index = 0;
    }

    fn update_read_search_matches(&mut self) {
        if self.read_search_query.is_empty() {
            self.read_search_matches.clear();
            self.read_search_index = 0;
            return;
        }
        let query = self.read_search_query.to_lowercase();
        self.read_search_matches = self
            .file_content
            .lines()
            .enumerate()
            .filter_map(|(i, line)| {
                if line.to_lowercase().contains(&query) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        self.read_search_index = 0;
    }

    // ── Scroll / position helpers ────────────────────────────────────

    fn max_read_scroll(&self) -> u16 {
        let line_count = self.file_content.lines().count().max(1);
        let viewport = self.core.content_area.height as usize;
        if line_count > viewport {
            (line_count - viewport) as u16
        } else {
            0
        }
    }

    fn line_to_scroll(line: usize) -> u16 {
        line.min(u16::MAX as usize) as u16
    }

    /// Convert a screen (col, row) to a buffer (line, char_col) position.
    pub fn screen_to_char_pos(
        editor: &EditorContext,
        area: ratatui::layout::Rect,
        screen_col: u16,
        screen_row: u16,
    ) -> Option<(usize, usize)> {
        let rope = editor.buffer.rope();
        let total_lines = rope.len_lines();
        let gutter_w = text::gutter_width(total_lines);
        let (line, display_col) = text::screen_to_buffer_pos(
            screen_col,
            screen_row,
            area.x,
            area.y,
            editor.state.scroll_offset,
            gutter_w,
        );
        if line >= total_lines {
            return None;
        }
        let line_text: String = rope.line(line).chars().collect();
        let content = line_text.trim_end_matches(&['\n', '\r'][..]);
        let char_col = text::display_col_to_char_col(content, display_col);
        Some((line, char_col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jones_workspace::WorkspaceEntryKind;
    use std::fs;
    use std::path::Path;

    /// Helper: create a temporary directory with the given entries.
    /// Each entry is (name, is_dir).
    struct TempDir {
        dir: tempfile::TempDir,
    }

    impl TempDir {
        fn new(entries: &[(&str, bool)]) -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            for (name, is_dir) in entries {
                let path = dir.path().join(name);
                if *is_dir {
                    fs::create_dir(&path).unwrap();
                } else {
                    fs::write(&path, "content").unwrap();
                }
            }
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn enter(path: &Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    #[test]
    fn list_entries_includes_parent_and_sorted_children() {
        let tmp = TempDir::new(&[
            ("zebra.txt", false),
            ("alpha.md", false),
            ("subdir", true),
            ("beta.rs", false),
        ]);
        let entries =
            workspace::list_workspace_entries(tmp.path(), &WorkspaceOptions::default(), &[]).0;
        // Expected: ParentDir, then directories (subdir), then files (alpha, beta, zebra).
        assert_eq!(entries.len(), 5, "should have parent + 1 dir + 3 files");

        // First entry is parent.
        assert_eq!(entries[0].kind, WorkspaceEntryKind::Parent);

        // Second is the directory.
        assert_eq!(entries[1].kind, WorkspaceEntryKind::Directory);
        assert_eq!(entries[1].name, "subdir");

        // Remaining are files sorted case-insensitively.
        let file_names: Vec<&str> = entries[2..]
            .iter()
            .map(|e| {
                assert_eq!(e.kind, WorkspaceEntryKind::File);
                e.name.as_str()
            })
            .collect();
        assert_eq!(file_names, ["alpha.md", "beta.rs", "zebra.txt"]);
    }

    #[test]
    fn list_entries_empty_directory() {
        let tmp = TempDir::new(&[]);
        let entries =
            workspace::list_workspace_entries(tmp.path(), &WorkspaceOptions::default(), &[]).0;
        assert_eq!(entries.len(), 1, "only parent entry");
        assert_eq!(entries[0].kind, WorkspaceEntryKind::Parent);
    }

    #[test]
    fn list_entries_no_parent_at_root() {
        // `/` has no parent, so no ParentDir entry.
        let entries =
            workspace::list_workspace_entries(Path::new("/"), &WorkspaceOptions::default(), &[]).0;
        // We can't assert exact count (it's the real root), but we can check
        // there is no ParentDir entry.
        let has_parent = entries.iter().any(|e| e.kind == WorkspaceEntryKind::Parent);
        assert!(!has_parent, "root should not have a ParentDir entry");
    }

    #[test]
    fn pick_default_file_prefers_readme() {
        let tmp = TempDir::new(&[
            ("notes.txt", false),
            ("README.md", false),
            ("main.rs", false),
        ]);
        let picked = TermiteApp::pick_default_file(tmp.path());
        assert!(picked.is_some());
        let name = picked
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(name, "README.md");
    }

    #[test]
    fn pick_default_file_prefers_index_md() {
        // Since read_dir order is filesystem-dependent, test with a single
        // file that should be picked as an .md file.
        let tmp = TempDir::new(&[("index.md", false)]);
        let picked = TermiteApp::pick_default_file(tmp.path());
        let name = picked
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(name, "index.md");
    }

    #[test]
    fn pick_default_file_prefers_readme_md() {
        let tmp = TempDir::new(&[("not_readme.txt", false), ("readme.md", false)]);
        let picked = TermiteApp::pick_default_file(tmp.path());
        let name = picked
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // readme.md should be preferred over .txt files even if encountered later.
        assert_eq!(name, "readme.md");
    }

    #[test]
    fn pick_default_file_falls_back_to_first_file() {
        let tmp = TempDir::new(&[("data.json", false), ("config.toml", false)]);
        let picked = TermiteApp::pick_default_file(tmp.path());
        let name = picked
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // First file encountered (sorted by read_dir order, not guaranteed),
        // but it should be one of the two.
        assert!(
            name == "data.json" || name == "config.toml",
            "should pick an existing file, got {name}"
        );
    }

    #[test]
    fn pick_default_file_empty_dir_returns_none() {
        let tmp = TempDir::new(&[]);
        let picked = TermiteApp::pick_default_file(tmp.path());
        assert!(picked.is_none());
    }

    #[test]
    fn scroll_keeps_selection_visible() {
        // We can't easily construct a full TermiteApp, but we can test
        // scroll_workspace_to_selection on a minimal state using a helper.
        let mut app = dummy_app();
        app.workspace_entries = vec![
            WorkspaceEntry::parent(),
            test_file("a"),
            test_file("b"),
            test_file("c"),
            test_file("d"),
            test_file("e"),
        ];
        app.workspace_selection = 4;
        app.workspace_scroll = 2;
        app.workspace_viewport_rows = 3;

        // Selection 4 is within viewport [2, 2+3) = [2, 5), so scroll stays.
        app.scroll_workspace_to_selection();
        assert_eq!(app.workspace_scroll, 2);

        // Move selection to 1, which is above scroll 2.
        app.workspace_selection = 1;
        app.scroll_workspace_to_selection();
        assert_eq!(app.workspace_scroll, 1);

        // Move selection to 5, which is at or beyond scroll+viewport (2+3=5).
        app.workspace_selection = 5;
        app.scroll_workspace_to_selection();
        // Should scroll so selection is visible: scroll = sel - (viewport - 1) = 5 - 2 = 3.
        assert_eq!(app.workspace_scroll, 3);
    }

    #[test]
    fn scroll_clamps_selection_to_zero() {
        let mut app = dummy_app();
        app.workspace_entries = vec![WorkspaceEntry::parent(), test_file("a")];
        app.workspace_selection = 0;
        app.workspace_scroll = 1;
        app.workspace_viewport_rows = 2;
        app.scroll_workspace_to_selection();
        assert_eq!(app.workspace_scroll, 0);
    }

    #[test]
    fn try_save_returns_true_when_not_dirty() {
        let mut app = dummy_app();
        // No editor => not dirty => should return true.
        assert!(app.try_save_before_switch());
    }

    #[test]
    fn change_cwd_rejects_non_directory() {
        let mut app = dummy_app();
        let original_cwd = app.cwd.clone();
        let tmp = tempfile::TempDir::new().unwrap();
        let file_path = tmp.path().join("some_file.txt");
        fs::write(&file_path, "hello").unwrap();
        app.change_cwd(file_path);
        // Cwd should be unchanged.
        assert_eq!(app.cwd, original_cwd);
        // Should have an error notification.
        assert!(app.notification.is_some());
        let (msg, _, is_error) = app.notification.as_ref().unwrap();
        assert!(*is_error);
        assert!(msg.contains("Not a directory"));
    }

    #[test]
    fn cwd_sync_does_not_redirect_relative_opened_file_save() {
        let original = tempfile::TempDir::new().unwrap();
        let other = tempfile::TempDir::new().unwrap();
        let original_file = original.path().join("note.txt");
        let other_file = other.path().join("note.txt");
        fs::write(&original_file, "original").unwrap();

        let _cwd_guard = CwdGuard::enter(original.path());
        let mut app = dummy_app();
        app.core.config.workspace.sync_terminal_cwd = true;
        assert!(app.load_file(Path::new("note.txt")));
        assert_eq!(app.current_file_path.as_ref(), Some(&original_file));

        app.change_cwd(other.path().to_path_buf());
        app.editor = Some(EditorContext::from_content("updated"));
        app.save_editor_file();

        assert_eq!(fs::read_to_string(&original_file).unwrap(), "updated");
        assert!(!other_file.exists());
        assert_eq!(
            app.file_snapshot.as_ref().map(|snap| &snap.path),
            Some(&original_file)
        );
    }

    #[test]
    fn sidebar_directory_enter_opens_browser_without_changing_cwd() {
        let tmp = TempDir::new(&[("subdir", true)]);
        let mut app = dummy_app();
        app.cwd = tmp.path().to_path_buf();
        app.workspace_entries = vec![test_dir("subdir")];
        app.workspace_selection = 0;

        app.open_workspace_entry();

        assert_eq!(app.cwd, tmp.path());
        assert!(app.workspace_fullscreen);
        assert_eq!(app.core.focus, Focus::Sidebar);
    }

    #[test]
    fn fullscreen_directory_enter_changes_cwd() {
        let tmp = TempDir::new(&[("subdir", true)]);
        let expected = tmp.path().join("subdir").canonicalize().unwrap();
        let mut app = dummy_app();
        app.cwd = tmp.path().to_path_buf();
        app.workspace_entries = vec![test_dir("subdir")];
        app.workspace_selection = 0;
        app.workspace_fullscreen = true;

        app.open_workspace_entry();

        assert_eq!(app.cwd, expected);
        assert!(app.workspace_fullscreen);
    }

    #[test]
    fn osc7_sequence_percent_encodes_spaces() {
        let sequence = terminal_cwd_osc7_sequence(Path::new("/tmp/a dir"));

        assert!(sequence.starts_with("\x1b]7;file://"));
        assert!(sequence.contains("/tmp/a%20dir"));
        assert!(sequence.ends_with('\u{7}'));
    }

    #[test]
    fn osc7_path_keeps_relative_paths_relative() {
        assert_eq!(percent_encode_path(Path::new("docs/a dir")), "docs/a%20dir");
    }

    #[test]
    fn osc7_path_formats_windows_drive_as_file_uri_path() {
        let encoded = percent_encode_path(Path::new(r"C:\Users\Cole\a dir"));

        assert_eq!(encoded, "/C:/Users/Cole/a%20dir");
    }

    fn test_file(name: &str) -> WorkspaceEntry {
        WorkspaceEntry {
            name: name.into(),
            kind: WorkspaceEntryKind::File,
            size: None,
            modified: None,
            extension: None,
            recent_rank: None,
        }
    }

    fn test_dir(name: &str) -> WorkspaceEntry {
        WorkspaceEntry {
            name: name.into(),
            kind: WorkspaceEntryKind::Directory,
            size: None,
            modified: None,
            extension: None,
            recent_rank: None,
        }
    }

    /// Build a minimal TermiteApp for unit tests that don't need a real terminal.
    fn dummy_app() -> TermiteApp {
        let config = Config::default();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut core = CoreState::new(config);
        core.focus = Focus::Content;
        TermiteApp {
            core,
            file_content: String::new(),
            file_scroll: 0,
            content_mode: ContentMode::Read,
            editor: None,
            current_file_path: None,
            highlighter: Highlighter::for_path(None),
            notification: None,
            file_modified_externally: false,
            read_search_active: false,
            read_search_query: String::new(),
            read_search_matches: Vec::new(),
            read_search_index: 0,
            outline_entries: Vec::new(),
            outline_active: false,
            outline_filter: String::new(),
            outline_selection: 0,
            line_jump_active: false,
            line_jump_buffer: String::new(),
            project_search_active: false,
            project_search_query: String::new(),
            project_search_results: Vec::new(),
            project_search_selection: 0,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            mouse_drag_active: false,
            last_click: None,
            last_click_pos: None,
            cwd,
            workspace_entries: Vec::new(),
            workspace_summary: workspace::WorkspaceSummary::default(),
            workspace_options: WorkspaceOptions::default(),
            workspace_filter_active: false,
            workspace_fullscreen: false,
            workspace_fullscreen_previous_focus: Focus::Content,
            recent_files: Vec::new(),
            workspace_preview_cache: HashMap::new(),
            workspace_selection: 0,
            workspace_scroll: 0,
            workspace_viewport_rows: 6,
            cwd_input_active: false,
            cwd_input_buffer: String::new(),
            file_snapshot: None,
            cached_word_count: 0,
            cached_rendered: None,
            cached_render_content_len: usize::MAX,
            needs_redraw: false,
        }
    }
}
