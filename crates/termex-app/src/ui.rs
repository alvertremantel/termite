use crate::app::{DocumentKind, Prompt, TermexApp, TermexMode};
use jones_text as text;
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

pub fn draw(frame: &mut Frame, app: &mut TermexApp) {
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme::base_style()),
        frame.area(),
    );

    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
    app.content_area = chunks[0];

    match app.mode {
        TermexMode::Read => draw_read(frame, app, chunks[0]),
        TermexMode::Write => draw_write(frame, app, chunks[0]),
    }
    draw_status(frame, app, chunks[1]);

    if app.help_visible {
        let area = jones_tui::centered_rect(58, 70, frame.area());
        draw_help(frame, area);
    }
    if app.prompt.is_some() {
        let area = jones_tui::centered_rect(62, 18, frame.area());
        draw_prompt(frame, app, area);
    }
}

fn draw_read(frame: &mut Frame, app: &mut TermexApp, area: Rect) {
    let title = format!(" {} ", app.title());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::md_accent()))
        .style(Style::default().bg(theme::bg_dark()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (content_area, search_area) = if app.read_search_active && inner.height > 1 {
        (
            Rect::new(inner.x, inner.y, inner.width, inner.height - 1),
            Some(Rect::new(
                inner.x,
                inner.y + inner.height - 1,
                inner.width,
                1,
            )),
        )
    } else {
        (inner, None)
    };

    let rendered = match app.document_kind {
        DocumentKind::Markdown => app.rendered_markdown(),
        DocumentKind::Source | DocumentKind::Plain => render_source(app),
    };
    let column = centered_reader_column(content_area);
    let paragraph = Paragraph::new(rendered)
        .wrap(Wrap { trim: false })
        .scroll((app.read_scroll, 0));
    frame.render_widget(paragraph, column);

    if let Some(area) = search_area {
        draw_read_search(frame, app, area);
    }
}

fn centered_reader_column(area: Rect) -> Rect {
    let max_width = 96;
    if area.width <= max_width {
        return Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
    }
    let width = max_width;
    let x = area.x + (area.width - width) / 2;
    Rect::new(x, area.y, width, area.height)
}

fn render_source(app: &TermexApp) -> Text<'static> {
    let total_lines = app.content.lines().count().max(1);
    let gutter_w = text::gutter_width(total_lines) as usize;
    let mut lines = Vec::new();
    if app.content.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>width$} ", 1, width = gutter_w.saturating_sub(1)),
                Style::default().fg(theme::editor_gutter()),
            ),
            Span::styled("\u{2502} ", Style::default().fg(theme::border_unfocused())),
        ]));
        return Text::from(lines);
    }
    for (idx, line) in app.content.lines().enumerate() {
        let mut spans = vec![
            Span::styled(
                format!("{:>width$} ", idx + 1, width = gutter_w.saturating_sub(1)),
                Style::default().fg(theme::editor_gutter()),
            ),
            Span::styled("\u{2502} ", Style::default().fg(theme::border_unfocused())),
        ];
        for (style, text) in app.highlighter.highlight_line(line) {
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn draw_write(frame: &mut Frame, app: &mut TermexApp, area: Rect) {
    let dirty = if app.dirty() { " *" } else { "" };
    let title = format!(" {}{} ", app.title(), dirty);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent_yellow()))
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(editor) = &app.editor else {
        frame.render_widget(
            Paragraph::new("No editor buffer").style(Style::default().fg(theme::text_dim())),
            inner,
        );
        return;
    };

    let mut editor_area = inner;
    if app.file_modified_externally && editor_area.height > 1 {
        let warn = " File changed on disk. Ctrl-R reloads after confirmation. ";
        frame.render_widget(
            Line::from(Span::styled(
                pad_to_width(warn, editor_area.width),
                Style::default()
                    .fg(theme::notify_error_fg())
                    .bg(theme::notify_error_bg()),
            )),
            Rect::new(editor_area.x, editor_area.y, editor_area.width, 1),
        );
        editor_area = Rect::new(
            editor_area.x,
            editor_area.y + 1,
            editor_area.width,
            editor_area.height - 1,
        );
    }

    let search_active = editor.search_active;
    let replace_active = editor.replace_active;
    let bar_height = if search_active {
        if replace_active { 2 } else { 1 }
    } else {
        0
    };
    let text_area = if bar_height > 0 && editor_area.height > bar_height {
        Rect::new(
            editor_area.x,
            editor_area.y,
            editor_area.width,
            editor_area.height - bar_height,
        )
    } else {
        editor_area
    };

    draw_editor_lines(frame, app, text_area);

    if search_active && editor_area.height > bar_height {
        draw_editor_find(frame, app, editor_area, bar_height);
    }
}

fn draw_editor_lines(frame: &mut Frame, app: &TermexApp, area: Rect) {
    let Some(editor) = &app.editor else {
        return;
    };
    let rope = editor.buffer.rope();
    let state = &editor.state;
    let total_lines = rope.len_lines();
    let gutter_w = text::gutter_width(total_lines);
    let content_width = area.width.saturating_sub(gutter_w + 2) as usize;
    let selection = state.selection.as_ref().map(|sel| sel.normalized());

    for row in 0..area.height as usize {
        let line_idx = state.scroll_offset + row;
        let y = area.y + row as u16;
        if line_idx >= total_lines {
            frame.render_widget(
                Line::from(vec![
                    Span::styled(
                        format!("{:>width$} ", "~", width = (gutter_w - 1) as usize),
                        Style::default().fg(theme::editor_gutter()),
                    ),
                    Span::styled("\u{2502}", Style::default().fg(theme::border_unfocused())),
                ]),
                Rect::new(area.x, y, area.width, 1),
            );
            continue;
        }

        let cursor_line = line_idx == state.cursor_line;
        if cursor_line {
            frame.render_widget(
                ratatui::widgets::Block::default()
                    .style(Style::default().bg(theme::bg_highlight())),
                Rect::new(area.x, y, area.width, 1),
            );
        }
        let row_bg = cursor_line.then(theme::bg_highlight);
        let apply_bg = |style: Style| row_bg.map_or(style, |bg| style.bg(bg));

        let mut spans = Vec::new();
        spans.push(Span::styled(
            format!("{:>width$} ", line_idx + 1, width = (gutter_w - 1) as usize),
            apply_bg(
                Style::default()
                    .fg(if cursor_line {
                        theme::editor_gutter_active()
                    } else {
                        theme::editor_gutter()
                    })
                    .add_modifier(if cursor_line {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ));
        spans.push(Span::styled(
            "\u{2502}",
            apply_bg(Style::default().fg(theme::border_unfocused())),
        ));

        let raw_line: String = rope.line(line_idx).chars().collect();
        let display = raw_line.trim_end_matches(&['\n', '\r'][..]);
        let truncated = text::truncate_to_display_width(display, content_width);

        if let Some(((start_line, start_col), (end_line, end_col))) = selection {
            let line_len = truncated.chars().count();
            let sel_start = if line_idx == start_line { start_col } else { 0 };
            let sel_end = if line_idx == end_line {
                end_col.min(line_len)
            } else {
                line_len
            };
            if line_idx >= start_line && line_idx <= end_line && sel_start < sel_end {
                push_highlighted_segment(
                    &mut spans,
                    app,
                    &truncated[..text::nth_char_byte_offset(truncated, sel_start)],
                    apply_bg,
                );
                let selected_start = text::nth_char_byte_offset(truncated, sel_start);
                let selected_end = text::nth_char_byte_offset(truncated, sel_end);
                push_highlighted_segment_with_style(
                    &mut spans,
                    app,
                    &truncated[selected_start..selected_end],
                    |style| style.bg(theme::selection_bg()),
                );
                push_highlighted_segment(&mut spans, app, &truncated[selected_end..], apply_bg);
            } else {
                push_highlighted_segment(&mut spans, app, truncated, apply_bg);
            }
        } else {
            push_highlighted_segment(&mut spans, app, truncated, apply_bg);
        }

        frame.render_widget(Line::from(spans), Rect::new(area.x, y, area.width, 1));
    }
}

fn push_highlighted_segment(
    spans: &mut Vec<Span<'static>>,
    app: &TermexApp,
    segment: &str,
    style_fn: impl Fn(Style) -> Style,
) {
    push_highlighted_segment_with_style(spans, app, segment, style_fn);
}

fn push_highlighted_segment_with_style(
    spans: &mut Vec<Span<'static>>,
    app: &TermexApp,
    segment: &str,
    style_fn: impl Fn(Style) -> Style,
) {
    for (style, text) in app.highlighter.highlight_line(segment) {
        spans.push(Span::styled(text, style_fn(style)));
    }
}

fn draw_editor_find(frame: &mut Frame, app: &TermexApp, area: Rect, bar_height: u16) {
    let Some(editor) = &app.editor else {
        return;
    };
    let find_y = area.y + area.height - bar_height;
    draw_input_row(
        frame,
        Rect::new(area.x, find_y, area.width, 1),
        "Find: ",
        &editor.search_query,
        !editor.replace_focused,
    );
    if editor.replace_active {
        draw_input_row(
            frame,
            Rect::new(area.x, find_y + 1, area.width, 1),
            "Replace: ",
            &editor.replace_query,
            editor.replace_focused,
        );
    }
}

fn draw_input_row(frame: &mut Frame, area: Rect, prompt: &str, value: &str, focused: bool) {
    let cursor = if focused { "_" } else { "" };
    let display = format!("{prompt}{value}{cursor}");
    let remaining = area.width as usize;
    let line = Line::from(vec![
        Span::styled(
            prompt,
            Style::default()
                .fg(if focused {
                    theme::search_prompt()
                } else {
                    theme::text_dim()
                })
                .bg(theme::bg_dark()),
        ),
        Span::styled(
            value.to_string(),
            Style::default()
                .fg(theme::text_primary())
                .bg(theme::bg_dark()),
        ),
        Span::styled(
            cursor,
            Style::default()
                .fg(theme::search_prompt())
                .bg(theme::bg_dark()),
        ),
        Span::styled(
            " ".repeat(remaining.saturating_sub(display.len())),
            Style::default().bg(theme::bg_dark()),
        ),
    ]);
    frame.render_widget(line, area);
}

fn draw_read_search(frame: &mut Frame, app: &TermexApp, area: Rect) {
    let info = if app.read_search_query.is_empty() {
        String::new()
    } else if app.read_search_matches.is_empty() {
        " (no matches)".to_string()
    } else {
        format!(
            " ({}/{})",
            app.read_search_index + 1,
            app.read_search_matches.len()
        )
    };
    let value = format!("{}{}", app.read_search_query, info);
    draw_input_row(frame, area, "/ ", &value, true);
}

fn draw_status(frame: &mut Frame, app: &TermexApp, area: Rect) {
    let (line, col) = app.current_position();
    let dirty = if app.dirty() { " modified" } else { "" };
    let message = app
        .notification
        .as_ref()
        .map(|n| n.message.as_str())
        .unwrap_or(match app.mode {
            TermexMode::Read => "q quit  e write  / search  ? help",
            TermexMode::Write => "Esc read  Ctrl-S save  Ctrl-Q discard/quit  Ctrl-F find",
        });
    let prompt = match &app.prompt {
        Some(Prompt::SaveAs { .. }) => "save as",
        Some(Prompt::Dirty { .. }) => "unsaved changes",
        None => "",
    };
    let status = format!(
        " {} | {}{} | Ln {}, Col {} | {} {}",
        app.mode.label(),
        app.title(),
        dirty,
        line,
        col,
        prompt,
        message
    );
    frame.render_widget(
        Line::from(Span::styled(
            pad_to_width(&status, area.width),
            theme::status_bar_style(),
        )),
        area,
    );
}

fn draw_prompt(frame: &mut Frame, app: &TermexApp, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Termex ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent_yellow()))
        .style(Style::default().bg(theme::bg_surface()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = match &app.prompt {
        Some(Prompt::SaveAs { buffer, .. }) => vec![
            Line::from(Span::styled(
                "Save scratch buffer as:",
                Style::default().fg(theme::text_secondary()),
            )),
            Line::from(vec![
                Span::styled("> ", Style::default().fg(theme::accent_yellow())),
                Span::styled(buffer.clone(), Style::default().fg(theme::text_primary())),
                Span::styled("_", Style::default().fg(theme::accent_yellow())),
            ]),
            Line::from(Span::styled(
                "Enter save  Esc cancel",
                Style::default().fg(theme::text_dim()),
            )),
        ],
        Some(Prompt::Dirty { .. }) => vec![
            Line::from(Span::styled(
                "Unsaved changes",
                Style::default()
                    .fg(theme::accent_yellow())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("Ctrl-S save  Ctrl-Q discard  Esc cancel"),
        ],
        None => Vec::new(),
    };

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::help_border()))
        .style(Style::default().bg(theme::bg_surface()));
    let lines = vec![
        Line::from(Span::styled(
            "termex",
            Style::default()
                .fg(theme::md_accent())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("READ   q quit   e write   / search   j/k scroll   g/G top/bottom"),
        Line::from("WRITE  Esc read   Ctrl-S save   Ctrl-F find   Ctrl-Q discard/quit"),
        Line::from(""),
        Line::from("Dirty prompts require save, discard, or cancel."),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn pad_to_width(text: &str, width: u16) -> String {
    format!("{text:<width$}", width = width as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn read_view_renders_status_and_scratch_title() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = TermexApp::new(None).unwrap();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();

        assert!(rendered.contains("[scratch]"));
        assert!(rendered.contains("READ"));
    }

    #[test]
    fn dirty_prompt_renders_save_and_discard_choices() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = TermexApp::new(None).unwrap();
        app.prompt = Some(Prompt::Dirty {
            action: crate::app::DirtyAction::Quit,
        });

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();

        assert!(rendered.contains("Unsaved changes"));
        assert!(rendered.contains("Ctrl-S save"));
    }
}
