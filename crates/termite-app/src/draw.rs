use crate::app::TermiteApp;
use jones_state::Focus;
use jones_theme as theme;
use jones_tui as ui;
use jones_workspace::{self as workspace, WorkspaceEntryKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use termite_editor::ContentMode;

/// Sidebar width bounds. Keep enough room for file names without stealing the document.
const SIDEBAR_MIN_WIDTH: u16 = 28;
const SIDEBAR_MAX_WIDTH: u16 = 44;
const CONTENT_MIN_WIDTH_WITH_SIDEBAR: u16 = 24;

pub fn draw(frame: &mut Frame, app: &mut TermiteApp) {
    // Apply base background
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme::base_style()),
        frame.area(),
    );

    let outer = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
    let body = outer[0];
    let status = outer[1];

    if app.workspace_fullscreen {
        app.core.sidebar_area = body;
        app.core.content_area = Rect::default();
        draw_workspace_panel(frame, app, body, true);
        draw_status_bar(frame, app, status);
    } else {
        let sidebar_width = sidebar_width(body.width);
        if app.core.sidebar_visible && sidebar_width > 0 {
            let chunks =
                Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)])
                    .split(body);
            app.core.sidebar_area = chunks[0];
            app.core.content_area = chunks[1];
            draw_workspace_panel(frame, app, chunks[0], false);
            draw_content_panel(frame, app, chunks[1]);
        } else {
            app.core.sidebar_area = Rect::default();
            app.core.content_area = body;
            if app.core.focus == Focus::Sidebar {
                app.core.focus = Focus::Content;
            }
            draw_content_panel(frame, app, body);
        }

        draw_status_bar(frame, app, status);
    }

    // ── Modal overlays ──────────────────────────────────────────────

    if app.cwd_input_active {
        let area = ui::centered_rect(55, 20, frame.area());
        crate::ui::draw_cwd_input(frame, app, area);
    }
    if app.outline_active {
        let area = ui::centered_rect(70, 70, frame.area());
        crate::ui::draw_outline_overlay(frame, app, area);
    }
    if app.line_jump_active {
        let area = ui::centered_rect(35, 12, frame.area());
        crate::ui::draw_line_jump(frame, app, area);
    }
    if app.project_search_active {
        let area = ui::centered_rect(80, 70, frame.area());
        crate::ui::draw_project_results(frame, app, area);
    }

    if app.core.searching {
        let area = ui::centered_rect(50, 40, frame.area());
        let title = " Search (Esc: cancel) ".to_string();
        ui::draw_search_with_title(frame, &app.core.search_query, &[], 0, area, &title);
    }

    if app.core.help_visible {
        let area = ui::centered_rect(58, 75, frame.area());
        let config_path = dirs::config_dir()
            .map(|d| d.join("termite").join("config.toml"))
            .map(|p| format!("Config: {}", p.display()))
            .unwrap_or_else(|| "Config: ~/.config/termite/config.toml".to_string());
        let footer = format!(
            "{config_path}\nOptional: [workspace] sync_terminal_cwd = true for best-effort terminal cwd hints\ntermite v{} — a workspace-oriented terminal editor",
            env!("CARGO_PKG_VERSION")
        );
        ui::draw_help_with_footer(frame, area, &termite_help_sections(), &footer);
    }
}

fn sidebar_width(total_width: u16) -> u16 {
    if total_width < SIDEBAR_MIN_WIDTH + CONTENT_MIN_WIDTH_WITH_SIDEBAR {
        return 0;
    }

    let desired = (total_width / 3).clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
    desired.min(total_width.saturating_sub(CONTENT_MIN_WIDTH_WITH_SIDEBAR))
}

fn draw_content_panel(frame: &mut Frame, app: &mut TermiteApp, area: Rect) {
    let content_border_style = if app.core.focus == Focus::Content {
        Style::default().fg(theme::MD_ACCENT)
    } else {
        Style::default().fg(theme::UNFOCUSED)
    };

    match app.content_mode {
        ContentMode::Read => crate::ui::draw_content(frame, app, area, content_border_style),
        ContentMode::Edit => crate::ui::draw_editor(frame, app, area, content_border_style),
        ContentMode::Split => crate::ui::draw_split(frame, app, area, content_border_style),
    }
}

// ── Workspace panel ──────────────────────────────────────────────────

fn draw_workspace_panel(frame: &mut Frame, app: &mut TermiteApp, area: Rect, fullscreen: bool) {
    let sidebar_focused = fullscreen || app.core.focus == Focus::Sidebar;

    let border_style = if sidebar_focused {
        Style::default().fg(theme::ACCENT_CYAN)
    } else {
        Style::default().fg(theme::BORDER_UNFOCUSED)
    };

    let cwd_display = app.cwd.display().to_string();
    let title = if fullscreen {
        format!(" Directory Browser: {cwd_display} ")
    } else {
        format!(" Files: {cwd_display} ")
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::BG_SURFACE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 {
        return;
    }

    let footer_rows = if inner.height >= 4 { 2 } else { 0 };
    let visible_rows = inner.height.saturating_sub(footer_rows) as usize;
    app.workspace_viewport_rows = visible_rows;

    // Re-clamp scroll after possible viewport resize.
    app.scroll_workspace_to_selection();

    // Keep selection in view — auto-scroll.
    let sel = app.workspace_selection;
    let scroll = app.workspace_scroll as usize;

    if app.workspace_entries.is_empty() && visible_rows > 0 {
        let msg = if app.workspace_options.filter.is_empty() {
            "  (empty directory)"
        } else {
            "  (no matches)"
        };
        frame.render_widget(
            Line::from(Span::styled(msg, Style::default().fg(theme::TEXT_MUTED))),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }

    for row in 0..visible_rows {
        let idx = scroll + row;
        if idx >= app.workspace_entries.len() {
            break;
        }
        let y = inner.y + row as u16;
        let entry = &app.workspace_entries[idx];
        let is_selected = idx == sel;

        let (prefix, icon, name, style) = match entry.kind {
            WorkspaceEntryKind::Parent => {
                let s = if is_selected && sidebar_focused {
                    Style::default()
                        .fg(theme::ACCENT_YELLOW)
                        .bg(theme::BG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_YELLOW)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_DIM)
                };
                (">", " \u{2190}", "..", s)
            }
            WorkspaceEntryKind::Directory => {
                let s = if is_selected && sidebar_focused {
                    Style::default()
                        .fg(theme::DIR_COLOR)
                        .bg(theme::BG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(theme::DIR_COLOR)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::DIR_COLOR)
                };
                (" ", " \u{1F4C1}", entry.name.as_str(), s)
            }
            WorkspaceEntryKind::File => {
                let is_current = app
                    .current_file_path
                    .as_ref()
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == entry.name.as_str()));
                let s = if is_selected && sidebar_focused {
                    Style::default()
                        .fg(theme::ACCENT_BLUE_BRIGHT)
                        .bg(theme::BG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_BLUE_BRIGHT)
                        .add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default()
                        .fg(theme::ACCENT_GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::FILE_COLOR)
                };
                (" ", " \u{1F4C4}", entry.name.as_str(), s)
            }
        };

        let cursor = if is_selected && sidebar_focused {
            prefix
        } else {
            " "
        };

        // Truncate long names.
        let max_label = (inner.width as usize).saturating_sub(6);
        let label = truncate(name, max_label);

        let badge = if entry.recent_rank.is_some() {
            " ↺"
        } else {
            ""
        };
        let line = Line::from(vec![
            Span::styled(cursor, style),
            Span::styled(icon, style),
            Span::styled(format!(" {label}{badge}"), style),
        ]);
        frame.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
    }

    if inner.height >= 3 {
        let y = inner.y + inner.height.saturating_sub(2);
        let summary = format!(
            "{} dirs • {} files • sort:{} • scope:{}{}{}",
            app.workspace_summary.dirs_total,
            app.workspace_summary.files_total,
            app.workspace_options.sort_mode.label(),
            app.workspace_options.scope.label(),
            if app.workspace_options.show_hidden {
                " • hidden:on"
            } else {
                ""
            },
            if app.workspace_options.filter.is_empty() {
                ""
            } else {
                " • filtered"
            }
        );
        frame.render_widget(
            Line::from(Span::styled(
                truncate(&summary, inner.width as usize),
                Style::default().fg(theme::TEXT_DIM),
            )),
            Rect::new(inner.x, y, inner.width, 1),
        );
        let preview = if let Some(e) = app.selected_workspace_entry() {
            format!(
                "{} • {} • {}",
                e.extension.clone().unwrap_or_else(|| if e.is_dir() {
                    "dir".into()
                } else {
                    "file".into()
                }),
                workspace::format_size(e.size),
                workspace::format_age(e.modified)
            )
        } else if app.workspace_options.filter.is_empty() {
            "empty directory".to_string()
        } else {
            "no matches".to_string()
        };
        let snippet = app.workspace_preview();
        let footer = if snippet.is_empty() {
            preview
        } else {
            format!("{preview} — {snippet}")
        };
        frame.render_widget(
            Line::from(Span::styled(
                truncate(&footer, inner.width as usize),
                Style::default().fg(theme::TEXT_MUTED),
            )),
            Rect::new(inner.x, y + 1, inner.width, 1),
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

// ── Status bar ────────────────────────────────────────────────────────

fn draw_status_bar(frame: &mut Frame, app: &TermiteApp, area: Rect) {
    let filter_hint;
    let hints = if app.workspace_fullscreen {
        if app.workspace_filter_active {
            filter_hint = format!(
                "[browser filter] {}  Enter:keep  Esc:clear",
                app.workspace_options.filter
            );
            filter_hint.as_str()
        } else {
            "[browser] Enter:open/change dir  Esc/Tab/F2:close  Arrows/PgUp/PgDn  Backspace:parent  c:path  /:filter"
        }
    } else {
        match app.core.focus {
            Focus::Sidebar if app.workspace_filter_active => {
                filter_hint = format!(
                    "[filter] {}  Enter:keep  Esc:clear",
                    app.workspace_options.filter
                );
                filter_hint.as_str()
            }
            Focus::Sidebar => match app.content_mode {
                ContentMode::Read => {
                    "[sidebar] Tab:content  Enter:open file  F2:browser  Arrows:move  /:filter  Ctrl+B:hide"
                }
                ContentMode::Edit => {
                    "[sidebar] Ctrl+E:content  Enter:open file  F2:browser  Arrows:move  /:filter  Ctrl+B:hide"
                }
                ContentMode::Split => {
                    "[sidebar] Ctrl+E:content  Enter:open file  F2:browser  Arrows:move  /:filter  Ctrl+B:hide"
                }
            },
            Focus::Content => match app.content_mode {
                ContentMode::Read => {
                    "q:quit Tab:files F2:browser Ctrl+B:sidebar e:edit o:outline ::line C-f:find ?:help"
                }
                ContentMode::Edit => {
                    "Esc:read  C-s:save  C-p:split  C-f:find  C-z:undo  Ctrl+E:files  F2:browser"
                }
                ContentMode::Split => {
                    "Esc:read  C-s:save  C-p:edit  C-f:find  C-z:undo  Ctrl+E:files  F2:browser"
                }
            },
        }
    };

    let status = build_termite_status(app, hints, area.width);
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(theme::STATUS_FG).bg(theme::STATUS_BG)),
        area,
    );
}

/// Build status bar with filename, dirty flag, and position info.
fn build_termite_status(app: &TermiteApp, hints: &str, width: u16) -> Line<'static> {
    let badge_bg = match app.content_mode {
        ContentMode::Read => theme::ACCENT_CYAN,
        ContentMode::Edit => theme::ACCENT_YELLOW,
        ContentMode::Split => theme::ACCENT_MAGENTA,
    };

    let filename = app
        .current_file_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "no file".to_string());

    let badge = format!(" {} ", app.content_mode.as_str());
    let prefix = format!(" {filename} ");

    // Compute space for hints
    let reserved = badge.len() + prefix.len() + 20;
    let hints_budget = (width as usize).saturating_sub(reserved);
    let display_hints = if hints.len() <= hints_budget {
        hints.to_string()
    } else if hints_budget > 3 {
        let cut = &hints[..hints_budget.saturating_sub(1)];
        let boundary = cut.rfind("  ").unwrap_or(cut.len());
        format!("{}…", &hints[..boundary])
    } else {
        String::new()
    };

    // Check for notification
    let notif_span = if let Some((msg, _, is_error)) = &app.notification {
        let (fg, bg) = if *is_error {
            (theme::NOTIFY_ERROR_FG, theme::NOTIFY_ERROR_BG)
        } else {
            (theme::NOTIFY_SUCCESS_FG, theme::NOTIFY_SUCCESS_BG)
        };
        Span::styled(format!(" {msg} "), Style::default().fg(fg).bg(bg))
    } else {
        Span::raw("")
    };

    // Check for dirty indicator
    let dirty_span = if matches!(app.content_mode, ContentMode::Edit | ContentMode::Split)
        && app.editor.as_ref().is_some_and(|e| e.is_dirty())
    {
        Span::styled(" [modified] ", Style::default().fg(theme::ACCENT_YELLOW))
    } else {
        Span::raw("")
    };

    // Word count indicator (uses pre-computed cache)
    let wc_text = if app.current_file_path.is_some() {
        format!("{}w ", app.cached_word_count)
    } else {
        String::new()
    };

    // Position indicator (right-aligned)
    let pos_text = match app.content_mode {
        ContentMode::Edit | ContentMode::Split => {
            if let Some(ed) = &app.editor {
                format!(
                    "Ln {}, Col {} ",
                    ed.state.cursor_line + 1,
                    ed.state.cursor_col + 1
                )
            } else {
                String::new()
            }
        }
        ContentMode::Read => {
            if app.current_file_path.is_some() {
                let total_lines = app.file_content.lines().count().max(1);
                let pct = ((app.file_scroll as usize + 1) * 100) / total_lines;
                let pct = pct.min(100);
                format!("Line {} ({pct}%) ", app.file_scroll + 1)
            } else {
                String::new()
            }
        }
    };

    let dirty_len = if matches!(app.content_mode, ContentMode::Edit | ContentMode::Split)
        && app.editor.as_ref().is_some_and(|e| e.is_dirty())
    {
        " [modified] ".len()
    } else {
        0
    };

    let notif_len = if let Some((msg, _, _)) = &app.notification {
        msg.len() + 2 // " {msg} "
    } else {
        0
    };

    let total_used = badge.len()
        + prefix.len()
        + 2
        + display_hints.len()
        + 1
        + dirty_len
        + notif_len
        + wc_text.len()
        + pos_text.len();

    let gap = (width as usize).saturating_sub(total_used);
    let gap_span = Span::styled(" ".repeat(gap), Style::default().bg(theme::STATUS_BG));

    Line::from(vec![
        Span::styled(
            badge,
            Style::default()
                .fg(theme::BG_DARK)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            prefix,
            Style::default()
                .fg(theme::TEXT_SECONDARY)
                .bg(theme::STATUS_BG),
        ),
        Span::styled(
            format!("  {display_hints} "),
            Style::default()
                .fg(theme::TEXT_SECONDARY)
                .bg(theme::STATUS_BG),
        ),
        dirty_span,
        notif_span,
        gap_span,
        Span::styled(
            wc_text,
            Style::default().fg(theme::TEXT_DIM).bg(theme::STATUS_BG),
        ),
        Span::styled(
            pos_text,
            Style::default()
                .fg(theme::TEXT_SECONDARY)
                .bg(theme::STATUS_BG),
        ),
    ])
}

// ── Help ──────────────────────────────────────────────────────────────

fn termite_help_sections() -> Vec<ui::HelpSection> {
    vec![
        ui::HelpSection {
            title: "Files and directories".to_string(),
            entries: vec![
                (
                    "  Tab       ".to_string(),
                    "Switch between content and the sidebar (read mode)".to_string(),
                ),
                (
                    "  Ctrl+E    ".to_string(),
                    "Switch sidebar/content when editing".to_string(),
                ),
                (
                    "  Ctrl+B    ".to_string(),
                    "Show or hide the sidebar".to_string(),
                ),
                (
                    "  F2        ".to_string(),
                    "Open full-screen directory browser".to_string(),
                ),
                (
                    "  Arrows    ".to_string(),
                    "Move selection; Right/Enter opens".to_string(),
                ),
                (
                    "  Enter     ".to_string(),
                    "Open file; in F2 browser also changes folders".to_string(),
                ),
                (
                    "  Esc       ".to_string(),
                    "Close full-screen directory browser".to_string(),
                ),
                (
                    "  Bksp      ".to_string(),
                    "Parent directory (F2 browser only)".to_string(),
                ),
                (
                    "  / or f    ".to_string(),
                    "Filter entries by name or extension (Esc clears)".to_string(),
                ),
                (
                    "  s         ".to_string(),
                    "Cycle sort: alpha, recent, modified".to_string(),
                ),
                (
                    "  .         ".to_string(),
                    "Toggle hidden files".to_string(),
                ),
                (
                    "  o         ".to_string(),
                    "Cycle scope: all, files-only, dirs-only".to_string(),
                ),
                (
                    "  c         ".to_string(),
                    "Type directory path (F2 browser only)".to_string(),
                ),
                (
                    "  Home/End  ".to_string(),
                    "Jump to first / last entry".to_string(),
                ),
                (
                    "  PgUp/PgDn ".to_string(),
                    "Page through directory list".to_string(),
                ),
                (
                    "  j/k/g/G   ".to_string(),
                    "Also supported for keyboard-heavy users".to_string(),
                ),
            ],
        },
        ui::HelpSection {
            title: "Navigation".to_string(),
            entries: vec![
                ("  j/k       ".to_string(), "Scroll down / up".to_string()),
                ("  Space / b ".to_string(), "Page down / up".to_string()),
                (
                    "  d / u     ".to_string(),
                    "Half-page down / up".to_string(),
                ),
                (
                    "  g / G     ".to_string(),
                    "Jump to top / bottom".to_string(),
                ),
                ("  :         ".to_string(), "Jump to line".to_string()),
                (
                    "  o         ".to_string(),
                    "Open file outline / symbol list".to_string(),
                ),
                (
                    "  r         ".to_string(),
                    "Search references across workspace".to_string(),
                ),
                (
                    "  [ / ]     ".to_string(),
                    "Back / forward through jumps".to_string(),
                ),
                ("  ?         ".to_string(), "Toggle this help".to_string()),
                ("  q         ".to_string(), "Quit".to_string()),
            ],
        },
        ui::HelpSection {
            title: "Reading".to_string(),
            entries: vec![
                ("  e         ".to_string(), "Enter edit mode".to_string()),
                (
                    "  Ctrl+F    ".to_string(),
                    "Find in content (Enter: next, C-p: prev)".to_string(),
                ),
            ],
        },
        ui::HelpSection {
            title: "Editing".to_string(),
            entries: vec![
                ("  Arrows    ".to_string(), "Move cursor".to_string()),
                ("  Shift+Arr ".to_string(), "Extend selection".to_string()),
                ("  Ctrl+L/R  ".to_string(), "Jump word boundary".to_string()),
                (
                    "  Home/End  ".to_string(),
                    "Start / end of line".to_string(),
                ),
                ("  PgUp/Dn   ".to_string(), "Page up / down".to_string()),
                ("  Ctrl+A    ".to_string(), "Select all".to_string()),
                ("  Ctrl+C/X/V".to_string(), "Copy / Cut / Paste".to_string()),
                ("  Ctrl+Z/Y  ".to_string(), "Undo / Redo".to_string()),
                (
                    "  Tab       ".to_string(),
                    "Indent selection (Shift+Tab: outdent)".to_string(),
                ),
                (
                    "  Ctrl+Bksp ".to_string(),
                    "Delete word left (Ctrl+Del: right)".to_string(),
                ),
                (
                    "  Ctrl+F    ".to_string(),
                    "Find / replace (Tab: toggle, C-a: all)".to_string(),
                ),
                (
                    "  Ctrl+P    ".to_string(),
                    "Toggle split preview".to_string(),
                ),
                ("  Ctrl+S    ".to_string(), "Save file".to_string()),
                (
                    "  Ctrl+R    ".to_string(),
                    "Reload file from disk".to_string(),
                ),
                (
                    "  Esc       ".to_string(),
                    "Return to read mode (auto-saves)".to_string(),
                ),
                (
                    "            ".to_string(),
                    "Auto-closes: ( [ { \" `".to_string(),
                ),
                (
                    "            ".to_string(),
                    "Mouse: click, drag-select, double-click word".to_string(),
                ),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_handles_unicode_boundaries() {
        assert_eq!(truncate("résumé-📄.md", 8), "résumé-…");
        assert_eq!(truncate("📄📄📄", 2), "📄…");
        assert_eq!(truncate("abc", 0), "");
    }
}
