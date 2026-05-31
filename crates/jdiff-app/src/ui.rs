use jones_git_diff::{ChangedFile, DiffLine, DiffLineKind, DiffSource, FileChange, GitSnapshot};
use jones_state::Focus;
use jones_theme as theme;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{JdiffApp, ViewMode};

pub fn render(frame: &mut Frame, app: &JdiffApp) {
    let area = frame.area();
    let vertical = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    let body = vertical[0];

    if body.width >= 96 {
        let chunks = Layout::horizontal([Constraint::Length(38), Constraint::Min(20)]).split(body);
        render_files(frame, chunks[0], app);
        render_diff(frame, chunks[1], app);
    } else {
        let chunks = Layout::vertical([Constraint::Percentage(42), Constraint::Min(8)]).split(body);
        render_files(frame, chunks[0], app);
        render_diff(frame, chunks[1], app);
    }

    render_status(frame, vertical[1], app);

    if app.core.help_visible {
        let popup = jones_tui::centered_rect(70, 70, area);
        jones_tui::draw_help(frame, popup, &help_sections());
    }
}

fn render_files(frame: &mut Frame, area: Rect, app: &JdiffApp) {
    let focused = app.core.focus == Focus::Sidebar;
    let title = match &app.snapshot {
        Some(snapshot) => format!(" Files {} ", repo_label(snapshot)),
        None => String::from(" Files "),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(focus_border(focused))
        .style(theme::base_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(error) = &app.last_error {
        render_message(frame, inner, "Unable to load diff", error, true);
        return;
    }

    let Some(snapshot) = &app.snapshot else {
        render_message(frame, inner, "Loading repository", "", false);
        return;
    };

    if snapshot.files.is_empty() {
        render_message(
            frame,
            inner,
            "Working tree clean",
            "No staged, unstaged, or untracked changes.",
            false,
        );
        return;
    }

    let visible_files = app.visible_files();
    if visible_files.is_empty() {
        render_message(
            frame,
            inner,
            "No changes match filters",
            "Use s, u, or t to re-enable staged, unstaged, or untracked files.",
            false,
        );
        return;
    }

    let visible_rows = inner.height as usize;
    let start = app
        .selected_file
        .saturating_sub(visible_rows.saturating_sub(1));
    let items = visible_files
        .into_iter()
        .skip(start)
        .take(visible_rows)
        .enumerate()
        .map(|(offset, (_, file))| {
            let visible_index = start + offset;
            file_item(visible_index, file, visible_index == app.selected_file)
        })
        .collect::<Vec<_>>();

    frame.render_widget(List::new(items), inner);
}

fn render_diff(frame: &mut Frame, area: Rect, app: &JdiffApp) {
    let focused = app.core.focus == Focus::Content;
    let title = app
        .selected_file()
        .map(|file| format!(" Diff {} ", path_label(file)))
        .unwrap_or_else(|| String::from(" Diff "));

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(focus_border(focused))
        .style(theme::base_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(error) = &app.last_error {
        render_message(frame, inner, "Diff unavailable", error, true);
        return;
    }

    let Some(file) = app.selected_file() else {
        render_message(
            frame,
            inner,
            "No file selected",
            "The repository has no visible changes.",
            false,
        );
        return;
    };

    let lines = diff_lines(file, app.mode);
    let paragraph = Paragraph::new(Text::from(lines))
        .scroll((app.diff_scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn render_status(frame: &mut Frame, area: Rect, app: &JdiffApp) {
    let filters = format!(
        "staged:{} unstaged:{} untracked:{}",
        flag(app.include_staged),
        flag(app.include_unstaged),
        flag(app.include_untracked)
    );
    let hints = format!(
        "q quit  r refresh  m mode  s/u/t filters  ? help  {}  {}",
        filters, app.status
    );
    let line = jones_tui::build_status(app.mode.label(), app.core.focus, &hints, area.width);
    frame.render_widget(Paragraph::new(line), area);
}

fn file_item(index: usize, file: &ChangedFile, selected: bool) -> ListItem<'static> {
    let selection_style = if selected {
        Style::default()
            .fg(theme::text_bright())
            .bg(theme::selection_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text_primary())
    };
    let (insertions, deletions, binary) = change_stats(file);
    let change_style = if binary {
        Style::default().fg(theme::accent_magenta())
    } else if insertions > 0 && deletions > 0 {
        Style::default().fg(theme::accent_yellow())
    } else if insertions > 0 {
        Style::default().fg(theme::accent_green())
    } else if deletions > 0 {
        Style::default().fg(theme::notify_error_fg())
    } else {
        Style::default().fg(theme::text_secondary())
    };
    let marker = if selected { ">" } else { " " };
    let staged = if file.staged.is_some() { "S" } else { " " };
    let unstaged = if file.unstaged.is_some() { "U" } else { " " };

    ListItem::new(Line::from(vec![
        Span::styled(format!("{marker} "), selection_style),
        Span::styled(
            format!("{:>2} ", index + 1),
            Style::default().fg(theme::text_dim()),
        ),
        Span::styled(
            format!("[{staged}{unstaged}] "),
            Style::default().fg(theme::accent_cyan()),
        ),
        Span::styled(status_label(file), change_style),
        Span::raw(" "),
        Span::styled(path_label(file), selection_style),
        Span::styled(
            format!(" +{} -{}", insertions, deletions),
            Style::default().fg(theme::text_dim()),
        ),
    ]))
}

fn diff_lines(file: &ChangedFile, mode: ViewMode) -> Vec<Line<'static>> {
    if change_stats(file).2 {
        return vec![Line::from(Span::styled(
            "Binary file changed.",
            Style::default().fg(theme::accent_magenta()),
        ))];
    }

    if file.sections.is_empty() {
        return vec![Line::from(Span::styled(
            "No diff detail available in this mode.",
            Style::default().fg(theme::text_secondary()),
        ))];
    }

    let mut lines = Vec::new();
    if let Some(old_path) = old_path_label(file) {
        lines.push(Line::from(vec![
            Span::styled("renamed from ", Style::default().fg(theme::text_dim())),
            Span::styled(old_path, Style::default().fg(theme::accent_yellow())),
        ]));
    }

    for section in &file.sections {
        lines.push(source_header(section.source));
        for hunk in &section.hunks {
            lines.push(Line::from(Span::styled(
                hunk.header.clone(),
                Style::default()
                    .fg(theme::accent_cyan())
                    .add_modifier(Modifier::BOLD),
            )));
            match mode {
                ViewMode::Heavy => {
                    for line in &hunk.lines {
                        lines.push(diff_line(line));
                    }
                }
                ViewMode::Light => lines.extend(compact_hunk_lines(&hunk.lines)),
            }
        }
    }

    lines
}

fn compact_hunk_lines(lines: &[DiffLine]) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    let mut collapsed = 0usize;

    for line in lines {
        if line.kind == DiffLineKind::Context {
            collapsed += 1;
            continue;
        }

        if collapsed > 0 {
            rendered.push(collapsed_context_line(collapsed));
            collapsed = 0;
        }
        rendered.push(diff_line(line));
    }

    if collapsed > 0 {
        rendered.push(collapsed_context_line(collapsed));
    }

    rendered
}

fn collapsed_context_line(count: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled("          ", Style::default().fg(theme::text_dim())),
        Span::styled(
            format!("... {count} unchanged line(s)"),
            Style::default().fg(theme::text_dim()),
        ),
    ])
}

fn diff_line(line: &DiffLine) -> Line<'static> {
    let number = match (line.old_lineno, line.new_lineno) {
        (Some(old), Some(new)) => format!("{old:>4} {new:>4} "),
        (Some(old), None) => format!("{old:>4}      "),
        (None, Some(new)) => format!("     {new:>4} "),
        (None, None) => String::from("          "),
    };
    let (prefix, style) = match line.kind {
        DiffLineKind::Addition => ("+", Style::default().fg(theme::accent_green())),
        DiffLineKind::Deletion => ("-", Style::default().fg(theme::notify_error_fg())),
        DiffLineKind::Context => (" ", Style::default().fg(theme::text_primary())),
        DiffLineKind::NoNewline => ("\\", Style::default().fg(theme::text_secondary())),
    };

    Line::from(vec![
        Span::styled(number, Style::default().fg(theme::text_dim())),
        Span::styled(prefix, style),
        Span::styled(line.content.clone(), style),
    ])
}

fn source_header(source: DiffSource) -> Line<'static> {
    let label = match source {
        DiffSource::Staged => "staged",
        DiffSource::Unstaged => "unstaged",
    };
    Line::from(Span::styled(
        format!("-- {label} --"),
        Style::default()
            .fg(theme::accent_blue_bright())
            .add_modifier(Modifier::BOLD),
    ))
}

fn render_message(frame: &mut Frame, area: Rect, title: &str, detail: &str, error: bool) {
    let style = if error {
        Style::default().fg(theme::notify_error_fg())
    } else {
        Style::default().fg(theme::text_secondary())
    };
    let mut lines = vec![Line::from(Span::styled(
        title.to_owned(),
        style.add_modifier(Modifier::BOLD),
    ))];
    if !detail.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(detail.to_owned(), style)));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        area,
    );
}

fn focus_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme::border_focused())
    } else {
        Style::default().fg(theme::border_unfocused())
    }
}

fn repo_label(snapshot: &GitSnapshot) -> String {
    snapshot
        .repo_root
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repository")
        .to_owned()
}

fn path_label(file: &ChangedFile) -> String {
    file.path.display().to_string()
}

fn old_path_label(file: &ChangedFile) -> Option<String> {
    file.old_path
        .as_ref()
        .map(|path| path.display().to_string())
}

fn status_label(file: &ChangedFile) -> String {
    format!("{:?}", file.status)
}

fn change_stats(file: &ChangedFile) -> (u32, u32, bool) {
    [file.staged.as_ref(), file.unstaged.as_ref()]
        .into_iter()
        .flatten()
        .fold((0, 0, false), |(added, removed, binary), change| {
            let next_binary = binary || change.added.is_none() || change.removed.is_none();
            (
                added + count(change, |change| change.added),
                removed + count(change, |change| change.removed),
                next_binary,
            )
        })
}

fn count(change: &FileChange, field: impl FnOnce(&FileChange) -> Option<u32>) -> u32 {
    field(change).unwrap_or(0)
}

fn flag(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn help_sections() -> Vec<jones_tui::HelpSection> {
    vec![
        jones_tui::HelpSection {
            title: String::from("Navigation"),
            entries: vec![
                (
                    String::from("  j/k, arrows"),
                    String::from(" move selection or scroll diff"),
                ),
                (String::from("  Tab"), String::from(" switch focus")),
                (String::from("  g/G"), String::from(" jump to start or end")),
                (
                    String::from("  PgUp/PgDn"),
                    String::from(" page current pane"),
                ),
            ],
        },
        jones_tui::HelpSection {
            title: String::from("Diff"),
            entries: vec![
                (String::from("  r"), String::from(" refresh now")),
                (
                    String::from("  m"),
                    String::from(" toggle light/heavy detail"),
                ),
                (
                    String::from("  s/u/t"),
                    String::from(" toggle staged, unstaged, untracked"),
                ),
                (String::from("  q, Ctrl-C"), String::from(" quit")),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn status_bar_mentions_mode_and_filters() {
        let backend = TestBackend::new(160, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = test_app();
        app.status = String::from("clean");

        terminal.draw(|frame| render(frame, &app)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("light"));
        assert!(rendered.contains("staged:on"));
        assert!(rendered.contains("clean"));
    }

    fn test_app() -> JdiffApp {
        JdiffApp {
            core: jones_state::CoreState::new(()),
            root: jones_git_diff::RepoRoot::new("/repo"),
            launched_from: std::path::PathBuf::from("/repo"),
            snapshot: None,
            selected_file: 0,
            diff_scroll: 0,
            mode: crate::app::ViewMode::Light,
            include_staged: true,
            include_unstaged: true,
            include_untracked: true,
            last_refresh: None,
            status: String::new(),
            last_error: None,
            runner: jones_git_diff::StdGitRunner,
        }
    }
}
