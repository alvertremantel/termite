use color_eyre::{Result, eyre::Context};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use jones_event::{AppEvent, EventHandler};
use jones_syntax::Highlighter;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::text::Text;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use termite_editor::{EditorAction, EditorContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermexMode {
    Read,
    Write,
}

impl TermexMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "READ",
            Self::Write => "WRITE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Markdown,
    Source,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirtyAction {
    Quit,
    Read,
    Reload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    SaveAs {
        buffer: String,
        after_save: Option<DirtyAction>,
    },
    Dirty {
        action: DirtyAction,
    },
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub created_at: Instant,
    pub is_error: bool,
}

pub struct TermexApp {
    pub config: termex_config::Config,
    pub mode: TermexMode,
    pub path: Option<PathBuf>,
    pub content: String,
    pub read_scroll: u16,
    pub editor: Option<EditorContext>,
    pub highlighter: Highlighter,
    pub document_kind: DocumentKind,
    pub prompt: Option<Prompt>,
    pub notification: Option<Notification>,
    pub file_modified_externally: bool,
    pub help_visible: bool,
    pub read_search_active: bool,
    pub read_search_query: String,
    pub read_search_matches: Vec<usize>,
    pub read_search_index: usize,
    pub content_area: Rect,
    pub needs_redraw: bool,
    pub running: bool,
    content_version: u64,
    rendered_cache: Option<(u64, Text<'static>)>,
    file_snapshot: Option<FileSnapshot>,
}

impl TermexApp {
    pub fn new(maybe_path: Option<PathBuf>) -> Result<Self> {
        let config = termex_config::Config::load()?;
        let (path, content, file_snapshot) = match maybe_path {
            Some(path) => {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)
                        .wrap_err_with(|| format!("creating {}", parent.display()))?;
                }
                if !path.exists() {
                    std::fs::write(&path, "")
                        .wrap_err_with(|| format!("creating {}", path.display()))?;
                }
                let content = std::fs::read_to_string(&path)
                    .wrap_err_with(|| format!("reading {}", path.display()))?;
                let snapshot = Self::snapshot_for(&path);
                (Some(path), content, snapshot)
            }
            None => (None, String::new(), None),
        };

        let highlighter = classify_highlighter(path.as_deref(), &content);
        let document_kind = classify_document(path.as_deref(), &content, &highlighter);

        Ok(Self {
            config,
            mode: TermexMode::Read,
            path,
            content,
            read_scroll: 0,
            editor: None,
            highlighter,
            document_kind,
            prompt: None,
            notification: None,
            file_modified_externally: false,
            help_visible: false,
            read_search_active: false,
            read_search_query: String::new(),
            read_search_matches: Vec::new(),
            read_search_index: 0,
            content_area: Rect::default(),
            needs_redraw: true,
            running: true,
            content_version: 0,
            rendered_cache: None,
            file_snapshot,
        })
    }

    pub async fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        let (mut events, _event_tx) = EventHandler::<()>::new(Duration::from_millis(100));

        while self.running {
            self.poll_file_modified();
            if self.needs_redraw {
                terminal.draw(|frame| crate::ui::draw(frame, self))?;
                self.needs_redraw = false;
            }
            if let Some(ev) = events.next().await {
                self.handle_event(ev);
            }
        }
        Ok(())
    }

    pub fn rendered_markdown(&mut self) -> Text<'static> {
        if let Some((version, text)) = &self.rendered_cache
            && *version == self.content_version
        {
            return text.clone();
        }
        let rendered = jones_render::markdown::render_markdown(&self.content);
        self.rendered_cache = Some((self.content_version, rendered.clone()));
        rendered
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "[scratch]".to_string())
    }

    pub fn dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(EditorContext::is_dirty)
    }

    pub fn current_position(&self) -> (usize, usize) {
        match self.mode {
            TermexMode::Read => (self.read_scroll as usize + 1, 1),
            TermexMode::Write => self
                .editor
                .as_ref()
                .map(|editor| (editor.state.cursor_line + 1, editor.state.cursor_col + 1))
                .unwrap_or((1, 1)),
        }
    }

    pub fn line_count(&self) -> usize {
        match self.mode {
            TermexMode::Read => self.content.lines().count().max(1),
            TermexMode::Write => self
                .editor
                .as_ref()
                .map(|editor| editor.buffer.line_count())
                .unwrap_or(1),
        }
    }

    pub fn enter_write_mode(&mut self) {
        let mut editor = EditorContext::from_content(&self.content);
        editor.state.scroll_offset = self.read_scroll as usize;
        editor.state.cursor_line =
            (self.read_scroll as usize).min(editor.buffer.line_count().saturating_sub(1));
        self.editor = Some(editor);
        self.mode = TermexMode::Write;
        self.read_search_active = false;
        self.needs_redraw = true;
    }

    fn handle_event(&mut self, ev: AppEvent<()>) {
        match ev {
            AppEvent::Key(key) => {
                self.needs_redraw = true;
                self.handle_key(key);
            }
            AppEvent::Mouse(mouse) => {
                self.needs_redraw = true;
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.scroll_up(3),
                    MouseEventKind::ScrollDown => self.scroll_down(3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.help_visible = false;
                    }
                    _ => {}
                }
            }
            AppEvent::Tick => {
                if let Some(notification) = &self.notification
                    && notification.created_at.elapsed() > Duration::from_secs(3)
                {
                    self.notification = None;
                    self.needs_redraw = true;
                }
            }
            AppEvent::Resize(_, _) => self.needs_redraw = true,
            AppEvent::Custom(()) => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.help_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help_visible = false;
            }
            return;
        }

        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return;
        }

        if self.read_search_active {
            self.handle_read_search_key(key);
            return;
        }

        match self.mode {
            TermexMode::Read => self.handle_read_key(key),
            TermexMode::Write => self.handle_write_key(key),
        }
    }

    fn handle_read_key(&mut self, key: KeyEvent) {
        if jones_event::is_quit(&key) {
            self.running = false;
            return;
        }

        match key.code {
            KeyCode::Char('e') if key.modifiers.is_empty() => self.enter_write_mode(),
            KeyCode::Char('/') | KeyCode::Char('f')
                if key.modifiers.is_empty() || key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.read_search_active = true;
                self.read_search_query.clear();
                self.read_search_matches.clear();
                self.read_search_index = 0;
            }
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.path.is_none() {
                    self.prompt = Some(Prompt::SaveAs {
                        buffer: String::new(),
                        after_save: None,
                    });
                }
            }
            _ => self.handle_read_navigation(key),
        }
    }

    fn handle_write_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.dirty() {
                self.prompt = Some(Prompt::Dirty {
                    action: DirtyAction::Quit,
                });
            } else {
                self.running = false;
            }
            return;
        }
        if jones_event::is_quit(&key) {
            if self.dirty() {
                self.prompt = Some(Prompt::Dirty {
                    action: DirtyAction::Quit,
                });
            } else {
                self.running = false;
            }
            return;
        }

        let viewport_h = self.content_area.height.saturating_sub(2).max(1) as usize;
        let version_before = self.editor.as_ref().map(|editor| editor.buffer.version());
        let action = match &mut self.editor {
            Some(editor) => {
                editor.viewport_height = viewport_h;
                editor.handle_key(key)
            }
            None => return,
        };

        if self
            .editor
            .as_ref()
            .is_some_and(|editor| Some(editor.buffer.version()) != version_before)
        {
            self.content = self
                .editor
                .as_ref()
                .map(EditorContext::text)
                .unwrap_or_default();
            self.invalidate_content();
        }

        match action {
            EditorAction::ExitEditor => self.request_read_mode(),
            EditorAction::SaveFile => {
                self.save_or_prompt();
            }
            EditorAction::ReloadFile => self.request_reload(),
            EditorAction::ToggleSplitPreview => {
                self.notify("Termex has only read and write modes", false);
            }
            EditorAction::Find | EditorAction::None => {}
        }

        if let Some(editor) = &mut self.editor {
            editor.ensure_visible(viewport_h);
        }
    }

    fn request_read_mode(&mut self) {
        if self.dirty() {
            self.prompt = Some(Prompt::Dirty {
                action: DirtyAction::Read,
            });
        } else {
            self.finish_read_mode();
        }
    }

    fn request_reload(&mut self) {
        if self.dirty() {
            self.prompt = Some(Prompt::Dirty {
                action: DirtyAction::Reload,
            });
        } else {
            self.reload_from_disk();
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        match prompt {
            Prompt::SaveAs {
                mut buffer,
                after_save,
            } => match key.code {
                KeyCode::Esc => {
                    self.notify("Save canceled", false);
                }
                KeyCode::Enter => {
                    let trimmed = buffer.trim();
                    if trimmed.is_empty() {
                        self.notify("Save path is empty", true);
                        self.prompt = Some(Prompt::SaveAs { buffer, after_save });
                    } else {
                        self.path = Some(PathBuf::from(trimmed));
                        if self.save_current()
                            && let Some(action) = after_save
                        {
                            self.apply_dirty_action(action);
                        }
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.prompt = Some(Prompt::SaveAs { buffer, after_save });
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.prompt = Some(Prompt::SaveAs { buffer, after_save });
                }
                _ => self.prompt = Some(Prompt::SaveAs { buffer, after_save }),
            },
            Prompt::Dirty { action } => match key.code {
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.path.is_none() {
                        self.prompt = Some(Prompt::SaveAs {
                            buffer: String::new(),
                            after_save: Some(action),
                        });
                    } else if self.save_current() {
                        self.apply_dirty_action(action);
                    } else {
                        self.prompt = Some(Prompt::Dirty { action });
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('q')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.discard_editor_changes();
                    self.apply_dirty_action(action);
                }
                KeyCode::Esc => {
                    self.notify("Canceled", false);
                }
                _ => self.prompt = Some(Prompt::Dirty { action }),
            },
        }
    }

    fn apply_dirty_action(&mut self, action: DirtyAction) {
        match action {
            DirtyAction::Quit => self.running = false,
            DirtyAction::Read => self.finish_read_mode(),
            DirtyAction::Reload => self.reload_from_disk(),
        }
    }

    fn finish_read_mode(&mut self) {
        if let Some(editor) = &self.editor {
            self.content = editor.text();
            self.read_scroll = editor.state.scroll_offset.min(u16::MAX as usize) as u16;
        }
        self.invalidate_content();
        self.mode = TermexMode::Read;
    }

    fn discard_editor_changes(&mut self) {
        self.editor = Some(EditorContext::from_content(&self.content));
    }

    pub fn save_or_prompt(&mut self) -> bool {
        if self.path.is_none() {
            self.prompt = Some(Prompt::SaveAs {
                buffer: String::new(),
                after_save: None,
            });
            return false;
        }
        self.save_current()
    }

    fn save_current(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            self.notify(format!("Save failed: {err}"), true);
            return false;
        }
        let result = match &mut self.editor {
            Some(editor) => editor.save(&path).map(|()| editor.text()),
            None => std::fs::write(&path, &self.content)
                .map(|()| self.content.clone())
                .map_err(Into::into),
        };

        match result {
            Ok(content) => {
                self.content = content;
                self.file_snapshot = Self::snapshot_for(&path);
                self.file_modified_externally = false;
                self.highlighter = classify_highlighter(self.path.as_deref(), &self.content);
                self.document_kind =
                    classify_document(self.path.as_deref(), &self.content, &self.highlighter);
                self.invalidate_content();
                self.notify(format!("Saved {}", path.display()), false);
                true
            }
            Err(err) => {
                self.notify(format!("Save failed: {err}"), true);
                false
            }
        }
    }

    fn reload_from_disk(&mut self) {
        let Some(path) = self.path.clone() else {
            self.notify("Scratch buffer has no file to reload", true);
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.content = content;
                self.editor = Some(EditorContext::from_content(&self.content));
                self.file_snapshot = Self::snapshot_for(&path);
                self.file_modified_externally = false;
                self.highlighter = classify_highlighter(self.path.as_deref(), &self.content);
                self.document_kind =
                    classify_document(self.path.as_deref(), &self.content, &self.highlighter);
                self.invalidate_content();
                self.notify(format!("Reloaded {}", path.display()), false);
            }
            Err(err) => self.notify(format!("Reload failed: {err}"), true),
        }
    }

    fn handle_read_navigation(&mut self, key: KeyEvent) {
        let viewport_height = self.content_area.height.saturating_sub(2).max(1) as usize;
        if jones_event::is_nav_up(&key) {
            self.scroll_up(1);
        } else if jones_event::is_nav_down(&key) {
            self.scroll_down(1);
        } else if key.code == KeyCode::Char(' ') || key.code == KeyCode::PageDown {
            self.scroll_down(viewport_height);
        } else if key.code == KeyCode::Char('b') || key.code == KeyCode::PageUp {
            self.scroll_up(viewport_height);
        } else if key.code == KeyCode::Char('d') {
            self.scroll_down(viewport_height / 2);
        } else if key.code == KeyCode::Char('u') {
            self.scroll_up(viewport_height / 2);
        } else if key.code == KeyCode::Char('g') {
            self.read_scroll = 0;
        } else if key.code == KeyCode::Char('G') {
            self.read_scroll = self.max_read_scroll(viewport_height);
        }
        self.clamp_read_scroll(viewport_height);
    }

    fn scroll_up(&mut self, amount: usize) {
        match self.mode {
            TermexMode::Read => self.read_scroll = self.read_scroll.saturating_sub(amount as u16),
            TermexMode::Write => {
                if let Some(editor) = &mut self.editor {
                    editor.state.scroll_offset = editor.state.scroll_offset.saturating_sub(amount);
                }
            }
        }
    }

    fn scroll_down(&mut self, amount: usize) {
        match self.mode {
            TermexMode::Read => {
                self.read_scroll = self.read_scroll.saturating_add(amount as u16);
                self.clamp_read_scroll(self.content_area.height.saturating_sub(2).max(1) as usize);
            }
            TermexMode::Write => {
                if let Some(editor) = &mut self.editor {
                    editor.state.scroll_offset = editor.state.scroll_offset.saturating_add(amount);
                }
            }
        }
    }

    fn handle_read_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.read_search_active = false,
            KeyCode::Enter => self.next_read_match(),
            KeyCode::Backspace => {
                self.read_search_query.pop();
                self.update_read_matches();
            }
            KeyCode::Char(c) => {
                self.read_search_query.push(c);
                self.update_read_matches();
            }
            _ => {}
        }
    }

    fn update_read_matches(&mut self) {
        self.read_search_matches.clear();
        self.read_search_index = 0;
        if self.read_search_query.is_empty() {
            return;
        }
        for (idx, line) in self.content.lines().enumerate() {
            if line.contains(&self.read_search_query) {
                self.read_search_matches.push(idx);
            }
        }
        if let Some(&line) = self.read_search_matches.first() {
            self.read_scroll = line.min(u16::MAX as usize) as u16;
        }
    }

    fn next_read_match(&mut self) {
        if self.read_search_matches.is_empty() {
            return;
        }
        self.read_search_index = (self.read_search_index + 1) % self.read_search_matches.len();
        self.read_scroll =
            self.read_search_matches[self.read_search_index].min(u16::MAX as usize) as u16;
    }

    fn max_read_scroll(&self, viewport_height: usize) -> u16 {
        self.content
            .lines()
            .count()
            .saturating_sub(viewport_height)
            .min(u16::MAX as usize) as u16
    }

    fn clamp_read_scroll(&mut self, viewport_height: usize) {
        self.read_scroll = self.read_scroll.min(self.max_read_scroll(viewport_height));
    }

    fn poll_file_modified(&mut self) {
        let Some(snapshot) = &self.file_snapshot else {
            return;
        };
        let Ok(meta) = std::fs::metadata(&snapshot.path) else {
            return;
        };
        let Ok(modified) = meta.modified() else {
            return;
        };
        if modified != snapshot.modified && self.mode == TermexMode::Write {
            self.file_modified_externally = true;
        }
    }

    fn snapshot_for(path: &Path) -> Option<FileSnapshot> {
        path.metadata().ok().map(|meta| FileSnapshot {
            path: path.to_path_buf(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }

    fn invalidate_content(&mut self) {
        self.content_version = self.content_version.saturating_add(1);
        self.rendered_cache = None;
    }

    fn notify(&mut self, message: impl Into<String>, is_error: bool) {
        self.notification = Some(Notification {
            message: message.into(),
            created_at: Instant::now(),
            is_error,
        });
    }
}

pub fn classify_highlighter(path: Option<&Path>, content: &str) -> Highlighter {
    Highlighter::for_path_or_shebang(path, content.lines().next())
}

pub fn classify_document(
    path: Option<&Path>,
    content: &str,
    highlighter: &Highlighter,
) -> DocumentKind {
    if path.is_some_and(is_markdown_path) {
        DocumentKind::Markdown
    } else if matches!(
        highlighter,
        Highlighter::Python
            | Highlighter::Json
            | Highlighter::Toml
            | Highlighter::Rust
            | Highlighter::Shell
    ) || is_shell_path(path)
        || is_shell_shebang(content)
    {
        DocumentKind::Source
    } else {
        DocumentKind::Plain
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

fn is_shell_path(path: Option<&Path>) -> bool {
    path.and_then(|p| p.extension())
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "sh" | "bash"))
}

fn is_shell_shebang(content: &str) -> bool {
    content.lines().next().is_some_and(|line| {
        line.starts_with("#!") && (line.contains("sh") || line.contains("bash"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
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
    fn scratch_buffer_starts_in_read_mode() {
        let app = TermexApp::new(None).unwrap();
        assert_eq!(app.mode, TermexMode::Read);
        assert!(app.path.is_none());
        assert_eq!(app.title(), "[scratch]");
    }

    #[test]
    fn missing_file_is_created() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("notes.md");
        let app = TermexApp::new(Some(path.clone())).unwrap();

        assert!(path.exists());
        assert_eq!(app.path, Some(path));
        assert_eq!(app.document_kind, DocumentKind::Markdown);
    }

    #[test]
    fn read_mode_enters_write_mode() {
        let mut app = TermexApp::new(None).unwrap();
        app.handle_key(key(KeyCode::Char('e')));

        assert_eq!(app.mode, TermexMode::Write);
        assert!(app.editor.is_some());
    }

    #[test]
    fn dirty_escape_opens_prompt() {
        let mut app = TermexApp::new(None).unwrap();
        app.enter_write_mode();
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));

        assert_eq!(
            app.prompt,
            Some(Prompt::Dirty {
                action: DirtyAction::Read
            })
        );
    }

    #[test]
    fn scratch_save_requests_save_as_prompt() {
        let mut app = TermexApp::new(None).unwrap();
        app.enter_write_mode();
        assert!(!app.save_or_prompt());

        assert!(matches!(app.prompt, Some(Prompt::SaveAs { .. })));
    }

    #[test]
    fn dirty_scratch_save_as_applies_pending_read_action() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("scratch.md");
        let mut app = TermexApp::new(None).unwrap();
        app.enter_write_mode();
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        for ch in path.display().to_string().chars() {
            app.handle_key(key(KeyCode::Char(ch)));
        }
        app.handle_key(key(KeyCode::Enter));

        assert_eq!(app.mode, TermexMode::Read);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "x");
    }

    #[test]
    fn scratch_save_as_relative_filename_writes_in_current_dir() {
        let dir = TempDir::new().unwrap();
        let _cwd_guard = CwdGuard::enter(dir.path());
        let mut app = TermexApp::new(None).unwrap();
        app.content = "saved".to_string();
        app.path = Some(PathBuf::from("relative.md"));

        assert!(app.save_current());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("relative.md")).unwrap(),
            "saved"
        );
    }
}
