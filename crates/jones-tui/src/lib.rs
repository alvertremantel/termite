use jones_state::Focus;
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

pub fn build_status(mode_label: &str, focus: Focus, hints: &str, width: u16) -> Line<'static> {
    let focus_str = match focus {
        Focus::Sidebar => "sidebar",
        Focus::Content => "content",
    };
    let badge = format!(" {mode_label} ");
    let prefix = format!(" {focus_str} | ");
    let reserved = badge.len() + prefix.len() + 1;
    let hints_budget = (width as usize).saturating_sub(reserved);

    let display_hints = if hints.len() <= hints_budget {
        hints.to_string()
    } else if hints_budget > 3 {
        // Truncate at a word boundary if possible
        let cut = &hints[..hints_budget.saturating_sub(1)];
        let boundary = cut.rfind("  ").unwrap_or(cut.len());
        format!("{}…", &hints[..boundary])
    } else {
        String::new()
    };

    Line::from(vec![
        Span::styled(
            badge,
            Style::default()
                .fg(theme::status_badge_fg())
                .bg(theme::status_badge_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{prefix}{display_hints} "),
            Style::default()
                .fg(theme::text_secondary())
                .bg(theme::status_bg()),
        ),
    ])
}

pub struct HelpSection {
    pub title: String,
    pub entries: Vec<(String, String)>,
}

pub fn draw_help(frame: &mut Frame, area: Rect, sections: &[HelpSection]) {
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help (? or Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::help_border()))
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let heading = Style::default()
        .fg(theme::help_heading())
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme::help_key());
    let desc_style = Style::default().fg(theme::text_primary());

    let mut lines: Vec<Line> = Vec::new();
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(section.title.clone(), heading)));
        for (k, d) in &section.entries {
            lines.push(Line::from(vec![
                Span::styled(k.clone(), key_style),
                Span::styled(d.clone(), desc_style),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

pub fn draw_help_with_footer(
    frame: &mut Frame,
    area: Rect,
    sections: &[HelpSection],
    footer: &str,
) {
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help (? or Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::help_border()))
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let heading = Style::default()
        .fg(theme::help_heading())
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme::help_key());
    let desc_style = Style::default().fg(theme::text_primary());
    let footer_style = Style::default().fg(theme::text_dim());

    let mut lines: Vec<Line> = Vec::new();
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(section.title.clone(), heading)));
        for (k, d) in &section.entries {
            lines.push(Line::from(vec![
                Span::styled(k.clone(), key_style),
                Span::styled(d.clone(), desc_style),
            ]));
        }
    }

    // Footer: blank line + dim text (supports multi-line footers via '\n')
    lines.push(Line::from(""));
    for footer_line in footer.split('\n') {
        lines.push(Line::from(Span::styled(
            format!("  {footer_line}"),
            footer_style,
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

pub fn draw_search(
    frame: &mut Frame,
    query: &str,
    results: &[(usize, String)],
    selected: usize,
    area: Rect,
) {
    draw_search_with_title(
        frame,
        query,
        results,
        selected,
        area,
        " Search (Esc: cancel) ",
    );
}

pub fn draw_search_with_title(
    frame: &mut Frame,
    query: &str,
    results: &[(usize, String)],
    selected: usize,
    area: Rect,
    title: &str,
) {
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::search_border()))
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme::search_prompt())),
        Span::raw(query.to_string()),
        Span::styled("_", Style::default().fg(theme::search_prompt())),
    ]);
    let input_area = Rect { height: 1, ..inner };
    frame.render_widget(Paragraph::new(input_line), input_area);

    let results_area = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };

    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .take(results_area.height as usize)
        .map(|(i, (_, name))| {
            let style = if i == selected {
                Style::default()
                    .fg(theme::highlight())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let prefix = if i == selected { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(format!("{prefix}{name}"), style)))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, results_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn centered_rect_returns_middle_slice() {
        let area = Rect::new(0, 0, 100, 40);
        let centered = centered_rect(50, 50, area);

        assert_eq!(centered, Rect::new(25, 10, 50, 20));
    }

    #[test]
    fn build_status_truncates_hints_to_fit_width() {
        let line = build_status("READ", Focus::Content, "one  two  three  four  five", 24);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(text.contains("READ"));
        assert!(text.contains("content"));
        assert!(text.contains('…'));
    }

    #[test]
    fn draw_search_renders_title_and_query() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                draw_search(
                    frame,
                    "alp",
                    &[(0, "Alpha".into()), (1, "Beta".into())],
                    0,
                    Rect::new(0, 0, 40, 8),
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("Search"));
        assert!(rendered.contains("alp"));
        assert!(rendered.contains("Alpha"));
    }
}
