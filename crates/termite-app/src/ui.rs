use crate::app::TermiteApp;
use jones_render::markdown as md_render;
use jones_text as text;
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

pub fn draw_content(frame: &mut Frame, app: &mut TermiteApp, area: Rect, border_style: Style) {
    let base_title = app
        .current_file_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| format!(" {} ", n.to_string_lossy()))
        .unwrap_or_else(|| " Preview ".to_string());
    let title = app
        .current_breadcrumb()
        .map(|b| format!("{} — {} ", base_title.trim_end(), b))
        .unwrap_or(base_title);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BG_DARK));

    if app.file_content.is_empty() {
        let welcome = vec![
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "termite",
                Style::default()
                    .fg(theme::MD_ACCENT)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "A minimal terminal text editor",
                Style::default().fg(theme::TEXT_SECONDARY),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press e to enter edit mode, ? for help.",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let help = Paragraph::new(welcome)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(help, area);
        return;
    }

    // Use cached rendered markdown when content hasn't changed.
    let content_len = app.file_content.len();
    if app.cached_rendered.is_none() || app.cached_render_content_len != content_len {
        let fresh = md_render::render_markdown(&app.file_content);
        app.cached_rendered = Some(fresh);
        app.cached_render_content_len = content_len;
    }
    let rendered = app.cached_rendered.clone().unwrap();

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Reserve 1 row at the bottom for the search bar when active
    let (content_inner, search_bar_area) = if app.read_search_active && inner.height > 1 {
        let content = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
        let bar = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        (content, Some(bar))
    } else {
        (inner, None)
    };

    // Add 1-char horizontal padding inside the border
    let padded = Rect::new(
        content_inner.x.saturating_add(1),
        content_inner.y,
        content_inner.width.saturating_sub(2),
        content_inner.height,
    );
    let paragraph = Paragraph::new(rendered)
        .wrap(Wrap { trim: false })
        .scroll((app.file_scroll, 0));
    frame.render_widget(paragraph, padded);

    // Render the read-mode search bar
    if let Some(bar_area) = search_bar_area {
        let match_count = app.read_search_matches.len();
        let position_info = if app.read_search_query.is_empty() {
            String::new()
        } else if match_count == 0 {
            " (no matches)".to_string()
        } else {
            format!(" ({}/{}) ", app.read_search_index + 1, match_count)
        };

        let prompt_style = Style::default().fg(theme::SEARCH_PROMPT).bg(theme::BG_DARK);
        let text_style = Style::default().fg(theme::TEXT_PRIMARY).bg(theme::BG_DARK);
        let info_style = Style::default().fg(theme::TEXT_DIM).bg(theme::BG_DARK);
        let bg_style = Style::default().bg(theme::BG_DARK);

        let prompt = "/ ";
        let display = format!("{prompt}{}{position_info}", app.read_search_query);
        let remaining = (bar_area.width as usize).saturating_sub(display.len() + 1); // +1 for cursor

        let bar_line = Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::styled(app.read_search_query.clone(), text_style),
            Span::styled("_", prompt_style),
            Span::styled(position_info, info_style),
            Span::styled(" ".repeat(remaining), bg_style),
        ]);
        frame.render_widget(bar_line, bar_area);
    }
}

pub fn draw_outline_overlay(frame: &mut Frame, app: &TermiteApp, area: Rect) {
    frame.render_widget(Clear, area);
    let title = format!(
        " Outline (Esc cancel, Enter jump) [{}] ",
        app.outline_filter
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_FOCUSED))
        .style(Style::default().bg(theme::BG_SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let idxs = app.filtered_outline_indices();
    let mut lines = Vec::new();
    let visible_rows = inner.height as usize;
    let start = app
        .outline_selection
        .saturating_sub(visible_rows.saturating_sub(1));
    for (row, idx) in idxs.iter().skip(start).take(visible_rows).enumerate() {
        let absolute_row = start + row;
        let e = &app.outline_entries[*idx];
        let marker = if absolute_row == app.outline_selection {
            "> "
        } else {
            "  "
        };
        let indent = "  ".repeat(e.depth.saturating_sub(1));
        let style = if absolute_row == app.outline_selection {
            Style::default()
                .fg(theme::ACCENT_CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{:>4}  {indent}{}", e.line + 1, e.label),
            style,
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No outline entries",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn draw_line_jump(frame: &mut Frame, app: &TermiteApp, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Jump to line ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_FOCUSED))
        .style(Style::default().bg(theme::BG_SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(":", Style::default().fg(theme::SEARCH_PROMPT)),
            Span::raw(app.line_jump_buffer.clone()),
            Span::raw("_"),
        ])),
        inner,
    );
}

pub fn draw_project_results(frame: &mut Frame, app: &TermiteApp, area: Rect) {
    frame.render_widget(Clear, area);
    let title = format!(
        " Project references (Esc cancel, Enter open) [{}] ",
        app.project_search_query
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_FOCUSED))
        .style(Style::default().bg(theme::BG_SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = Vec::new();
    let visible_rows = inner.height as usize;
    let start = app
        .project_search_selection
        .saturating_sub(visible_rows.saturating_sub(1));
    for (row, r) in app
        .project_search_results
        .iter()
        .skip(start)
        .take(visible_rows)
        .enumerate()
    {
        let absolute_row = start + row;
        let marker = if absolute_row == app.project_search_selection {
            "> "
        } else {
            "  "
        };
        let rel = r.path.strip_prefix(&app.cwd).unwrap_or(&r.path);
        let style = if absolute_row == app.project_search_selection {
            Style::default()
                .fg(theme::ACCENT_CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{}:{}  {}", rel.display(), r.line + 1, r.preview),
            style,
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Type to search workspace text",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Find the nearest markdown heading at or above `cursor_line` in `text`.
/// Returns the heading text (without the leading `#` characters) if found.
fn nearest_heading_above(text: &str, cursor_line: usize) -> Option<String> {
    let mut result: Option<String> = None;
    for (i, line) in text.lines().enumerate() {
        if i > cursor_line {
            break;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
            let heading = trimmed[hashes..].trim();
            if !heading.is_empty() && hashes <= 6 {
                result = Some(heading.to_string());
            }
        }
    }
    result
}

/// Render the split view: editor on the left, live markdown preview on the right.
pub fn draw_split(frame: &mut Frame, app: &mut TermiteApp, area: Rect, border_style: Style) {
    let halves =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Left: editor
    draw_editor(frame, app, halves[0], border_style);

    // Right: live preview rendered from the editor buffer
    let preview_text = app.editor.as_ref().map(|ed| ed.text()).unwrap_or_default();

    // Build a contextual preview title showing the nearest heading above the cursor.
    let cursor_line = app
        .editor
        .as_ref()
        .map(|ed| ed.state.cursor_line)
        .unwrap_or(0);

    let title = match nearest_heading_above(&preview_text, cursor_line) {
        Some(heading) => format!(" Preview \u{2014} {heading} "),
        None => " Preview ".to_string(),
    };

    let preview_border = Style::default().fg(theme::ACCENT_MAGENTA);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(preview_border)
        .style(Style::default().bg(theme::BG_DARK));

    if preview_text.is_empty() {
        let help = Paragraph::new("Start typing to see a live preview.")
            .block(block)
            .style(Style::default().fg(theme::UNFOCUSED));
        frame.render_widget(help, halves[1]);
    } else {
        // Compute the preview viewport height (inner area minus borders).
        let preview_viewport_h = halves[1].height.saturating_sub(2) as usize;

        // Center the preview around the cursor position: place the cursor
        // roughly one-third from the top of the preview viewport so that
        // surrounding context is always visible.
        let scroll = cursor_line
            .saturating_sub(preview_viewport_h / 3)
            .min(u16::MAX as usize) as u16;

        let rendered = md_render::render_markdown(&preview_text);
        let paragraph = Paragraph::new(rendered)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        frame.render_widget(paragraph, halves[1]);
    }
}

/// Render the editor view with line numbers, syntax highlighting, cursor,
/// scrollbar indicator, and optional find bar.
pub fn draw_editor(frame: &mut Frame, app: &mut TermiteApp, area: Rect, border_style: Style) {
    let title = app
        .current_file_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| {
            let dirty = app.editor.as_ref().is_some_and(|ed| ed.is_dirty());
            let marker = if dirty { " *" } else { "" };
            format!(" {}{marker} ", n.to_string_lossy())
        })
        .unwrap_or_else(|| " Editor ".to_string());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme::BG_SURFACE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(editor) = &app.editor else {
        let help = Paragraph::new("No file loaded.").style(Style::default().fg(theme::UNFOCUSED));
        frame.render_widget(help, inner);
        return;
    };

    let search_active = editor.search_active;
    let search_query = editor.search_query.clone();
    let replace_active = editor.replace_active;
    let replace_query = editor.replace_query.clone();
    let replace_focused = editor.replace_focused;

    // Reserve rows at the bottom for the find bar (1 row) + replace bar (1 extra row).
    let bar_height: u16 = if search_active {
        if replace_active { 2 } else { 1 }
    } else {
        0
    };
    let editor_area = if bar_height > 0 && inner.height > bar_height {
        Rect::new(inner.x, inner.y, inner.width, inner.height - bar_height)
    } else {
        inner
    };

    // Show external-modification warning banner at the top of the editor area.
    let editor_area = if app.file_modified_externally && editor_area.height > 1 {
        let warn_area = Rect::new(editor_area.x, editor_area.y, editor_area.width, 1);
        let warn_msg =
            "\u{26a0} File modified externally. Press Ctrl+R to reload or continue editing.";
        let warn_style = Style::default()
            .fg(theme::NOTIFY_ERROR_FG)
            .bg(theme::NOTIFY_ERROR_BG);
        let padded: String = format!("{:<width$}", warn_msg, width = editor_area.width as usize);
        let warn_line = Line::from(Span::styled(padded, warn_style));
        frame.render_widget(warn_line, warn_area);
        Rect::new(
            editor_area.x,
            editor_area.y + 1,
            editor_area.width,
            editor_area.height - 1,
        )
    } else {
        editor_area
    };

    let rope = editor.buffer.rope();
    let ed = &editor.state;
    let total_lines = rope.len_lines();
    let gutter_w = text::gutter_width(total_lines);
    // Reserve 1 column on the right for the scrollbar.
    let text_width = editor_area.width.saturating_sub(gutter_w + 1 + 1) as usize; // +1 gutter sep, +1 scrollbar
    let viewport_height = editor_area.height as usize;
    let scrollbar_x = editor_area.x + editor_area.width.saturating_sub(1);

    let first_visible = ed.scroll_offset;

    // Compute selection bounds
    let selection_bounds = ed.selection.as_ref().map(|sel| sel.normalized());

    for view_row in 0..viewport_height {
        let line_idx = first_visible + view_row;
        let y = editor_area.y + view_row as u16;

        if line_idx >= total_lines {
            // Past end of file — tilde gutter
            let gutter_text = format!("{:>width$} ", "~", width = (gutter_w - 1) as usize,);
            let mut tilde_spans = vec![
                Span::styled(gutter_text, Style::default().fg(theme::EDITOR_GUTTER)),
                Span::styled("\u{2502}", Style::default().fg(theme::BORDER_UNFOCUSED)),
            ];
            // Pad remaining width so background fills (minus scrollbar column)
            let used = gutter_w as usize + 1; // gutter + separator
            let remaining = (editor_area.width as usize).saturating_sub(used + 1); // -1 for scrollbar
            if remaining > 0 {
                tilde_spans.push(Span::raw(" ".repeat(remaining)));
            }
            let line = Line::from(tilde_spans);
            frame.render_widget(
                line,
                Rect::new(editor_area.x, y, editor_area.width.saturating_sub(1), 1),
            );
            continue;
        }

        // Current-line highlight: fill the entire row with BG_HIGHLIGHT
        let is_cursor_line = line_idx == ed.cursor_line;
        if is_cursor_line {
            let highlight_block =
                ratatui::widgets::Block::default().style(Style::default().bg(theme::BG_HIGHLIGHT));
            frame.render_widget(
                highlight_block,
                Rect::new(editor_area.x, y, editor_area.width.saturating_sub(1), 1),
            );
        }

        let line_bg = if is_cursor_line {
            Some(theme::BG_HIGHLIGHT)
        } else {
            None
        };

        let mut spans: Vec<Span<'_>> = Vec::new();

        // ── Gutter ──────────────────────────────────────────────────
        let line_num = line_idx + 1;
        let mut gutter_style = if is_cursor_line {
            Style::default()
                .fg(theme::EDITOR_GUTTER_ACTIVE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::EDITOR_GUTTER)
        };
        if let Some(bg) = line_bg {
            gutter_style = gutter_style.bg(bg);
        }
        let gutter_text = format!("{:>width$} ", line_num, width = (gutter_w - 1) as usize,);
        spans.push(Span::styled(gutter_text, gutter_style));

        // ── Gutter separator ────────────────────────────────────────
        let mut sep_style = Style::default().fg(theme::BORDER_UNFOCUSED);
        if let Some(bg) = line_bg {
            sep_style = sep_style.bg(bg);
        }
        spans.push(Span::styled("\u{2502}", sep_style));

        // ── Line content with syntax highlighting ───────────────────
        let rope_line = rope.line(line_idx);
        let line_text: String = rope_line.chars().collect();
        let display_text = line_text.trim_end_matches(&['\n', '\r'][..]);
        let truncated = text::truncate_to_display_width(display_text, text_width);

        // Helper: apply cursor-line background to a style when applicable.
        let apply_bg = |s: Style| -> Style {
            match line_bg {
                Some(bg) => s.bg(bg),
                None => s,
            }
        };

        // Check for selection on this line
        if let Some(((sel_sl, sel_sc), (sel_el, sel_ec))) = selection_bounds {
            let line_char_len = truncated.chars().count();
            let sel_start = if line_idx == sel_sl { sel_sc } else { 0 };
            let sel_end = if line_idx == sel_el {
                sel_ec.min(line_char_len)
            } else {
                line_char_len
            };
            let in_selection = line_idx >= sel_sl && line_idx <= sel_el && sel_start < sel_end;

            if in_selection {
                let before_byte =
                    text::nth_char_byte_offset(truncated, sel_start.min(line_char_len));
                let sel_byte = text::nth_char_byte_offset(truncated, sel_end.min(line_char_len));
                let before = &truncated[..before_byte];
                let selected = &truncated[before_byte..sel_byte];
                let after = &truncated[sel_byte..];

                if !before.is_empty() {
                    for (style, text) in app.highlighter.highlight_line(before) {
                        spans.push(Span::styled(text, apply_bg(style)));
                    }
                }
                if !selected.is_empty() {
                    for (style, text) in app.highlighter.highlight_line(selected) {
                        spans.push(Span::styled(text, style.bg(theme::SELECTION_BG)));
                    }
                }
                if !after.is_empty() {
                    for (style, text) in app.highlighter.highlight_line(after) {
                        spans.push(Span::styled(text, apply_bg(style)));
                    }
                }
            } else {
                for (style, text) in app.highlighter.highlight_line(truncated) {
                    spans.push(Span::styled(text, apply_bg(style)));
                }
            }
        } else {
            for (style, text) in app.highlighter.highlight_line(truncated) {
                spans.push(Span::styled(text, apply_bg(style)));
            }
        }

        let line_widget = Line::from(spans);
        frame.render_widget(
            line_widget,
            Rect::new(editor_area.x, y, editor_area.width.saturating_sub(1), 1),
        );
    }

    // ── Scrollbar ───────────────────────────────────────────────────
    if viewport_height > 0 && total_lines > 0 {
        let vh = viewport_height as f64;
        let tl = total_lines.max(1) as f64;
        let thumb_height = (vh / tl * vh).max(1.0).min(vh) as usize;
        let scroll_fraction =
            ed.scroll_offset as f64 / (total_lines.saturating_sub(viewport_height).max(1)) as f64;
        let thumb_y = (scroll_fraction * (viewport_height.saturating_sub(thumb_height)) as f64)
            .round() as usize;

        let track_style = Style::default()
            .fg(theme::BORDER_UNFOCUSED)
            .bg(theme::BG_SURFACE);
        let thumb_style = Style::default().fg(theme::TEXT_DIM).bg(theme::BG_SURFACE);

        for row in 0..viewport_height {
            let y = editor_area.y + row as u16;
            let is_thumb = row >= thumb_y && row < thumb_y + thumb_height;
            let (ch, style) = if is_thumb {
                ("\u{2590}", thumb_style) // ▐
            } else {
                ("\u{2502}", track_style) // │
            };
            let span = Line::from(Span::styled(ch, style));
            frame.render_widget(span, Rect::new(scrollbar_x, y, 1, 1));
        }
    }

    // ── Find / Replace bar ────────────────────────────────────────
    if search_active && inner.height > bar_height {
        let find_y = inner.y + inner.height - bar_height;
        let find_area = Rect::new(inner.x, find_y, inner.width, 1);

        let find_prompt = "Find: ";
        let find_cursor = if !replace_focused { "_" } else { "" };
        let find_display = format!("{find_prompt}{search_query}{find_cursor}");
        let find_remaining = (inner.width as usize).saturating_sub(find_display.len());

        let find_prompt_style = if !replace_focused {
            Style::default().fg(theme::SEARCH_PROMPT).bg(theme::BG_DARK)
        } else {
            Style::default().fg(theme::TEXT_DIM).bg(theme::BG_DARK)
        };

        let find_line = Line::from(vec![
            Span::styled(find_prompt, find_prompt_style),
            Span::styled(
                search_query.clone(),
                Style::default().fg(theme::TEXT_PRIMARY).bg(theme::BG_DARK),
            ),
            Span::styled(find_cursor, find_prompt_style),
            Span::styled(
                " ".repeat(find_remaining),
                Style::default().bg(theme::BG_DARK),
            ),
        ]);
        frame.render_widget(find_line, find_area);

        if replace_active {
            let repl_y = find_y + 1;
            let repl_area = Rect::new(inner.x, repl_y, inner.width, 1);

            let repl_prompt = "Replace: ";
            let repl_cursor = if replace_focused { "_" } else { "" };
            let repl_display = format!("{repl_prompt}{replace_query}{repl_cursor}");
            let repl_remaining = (inner.width as usize).saturating_sub(repl_display.len());

            let repl_prompt_style = if replace_focused {
                Style::default().fg(theme::SEARCH_PROMPT).bg(theme::BG_DARK)
            } else {
                Style::default().fg(theme::TEXT_DIM).bg(theme::BG_DARK)
            };

            let repl_line = Line::from(vec![
                Span::styled(repl_prompt, repl_prompt_style),
                Span::styled(
                    replace_query.clone(),
                    Style::default().fg(theme::TEXT_PRIMARY).bg(theme::BG_DARK),
                ),
                Span::styled(repl_cursor, repl_prompt_style),
                Span::styled(
                    " ".repeat(repl_remaining),
                    Style::default().bg(theme::BG_DARK),
                ),
            ]);
            frame.render_widget(repl_line, repl_area);
        }

        // Place terminal cursor in the focused field.
        let (active_prompt_len, active_query_len, active_y) = if replace_focused && replace_active {
            ("Replace: ".len(), replace_query.len(), find_y + 1)
        } else {
            ("Find: ".len(), search_query.len(), find_y)
        };
        let cursor_x = inner.x + (active_prompt_len + active_query_len) as u16;
        if cursor_x < inner.x + inner.width {
            frame.set_cursor_position((cursor_x, active_y));
        }
        return; // Skip normal cursor positioning when find bar is active.
    }

    // ── Cursor positioning ──────────────────────────────────────────
    let cursor_display_col = if ed.cursor_line < total_lines {
        let cursor_rope_line = rope.line(ed.cursor_line);
        let cursor_line_text: String = cursor_rope_line.chars().collect();
        let cursor_display_text = cursor_line_text.trim_end_matches(&['\n', '\r'][..]);
        text::char_col_to_display_col(cursor_display_text, ed.cursor_col)
    } else {
        ed.cursor_col
    };

    if let Some((cx, cy)) = text::buffer_to_screen_pos(
        ed.cursor_line,
        cursor_display_col,
        ed.scroll_offset,
        gutter_w + 1, // +1 for gutter separator
        editor_area.x,
        editor_area.y,
        editor_area.height,
    ) && cx < editor_area.x + editor_area.width.saturating_sub(1)
        && cy < editor_area.y + editor_area.height
    {
        frame.set_cursor_position((cx, cy));
    }
}

/// Draw a lightweight prompt for typing a target directory path to change cwd.
pub fn draw_cwd_input(frame: &mut Frame, app: &TermiteApp, area: Rect) {
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Change directory (Esc: cancel) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_FOCUSED))
        .style(Style::default().bg(theme::BG_SURFACE));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    // Prompt label
    let prompt_line = Line::from(Span::styled(
        "Enter a directory path:",
        Style::default().fg(theme::TEXT_SECONDARY),
    ));
    frame.render_widget(
        Paragraph::new(prompt_line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Input field
    let input_y = inner.y + 1;
    let input = app.cwd_input_buffer.clone();
    let display = if input.is_empty() {
        Span::styled("_", Style::default().fg(theme::TEXT_DIM))
    } else {
        Span::styled(
            format!("{input}_"),
            Style::default().fg(theme::TEXT_PRIMARY),
        )
    };
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme::SEARCH_PROMPT)),
        display,
    ]);
    frame.render_widget(
        Paragraph::new(input_line),
        Rect::new(inner.x, input_y, inner.width, 1),
    );

    // Hint
    let hint_y = inner.y + 2;
    let hint_text = if inner.height > 3 {
        "Type a path (absolute or relative), then press Enter. Esc to cancel."
    } else {
        "Enter: confirm  Esc: cancel"
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            hint_text,
            Style::default().fg(theme::TEXT_MUTED),
        )),
        Rect::new(inner.x, hint_y, inner.width, 1),
    );
}
