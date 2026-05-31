use crate::app::{AzideApp, ExploreFocus, RssView};
use crate::settings::AppMode;
use jones_state::Focus;
use jones_theme as theme;
use jones_tui as ui;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn draw(frame: &mut Frame, app: &mut AzideApp) {
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(frame.area());

    // Fill background
    frame.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(theme::bg_dark())),
        frame.area(),
    );

    if app.mode == AppMode::Settings {
        // Settings takes the full body area
        app.core.content_area = chunks[0];
        crate::settings::draw_settings(frame, app, chunks[0]);

        // Draw create view modal if active
        if app.creating_view {
            let area = ui::centered_rect(50, 50, frame.area());
            crate::ui::draw_create_view_modal(frame, app, area);
        }

        // Settings status bar
        let hints = if app.settings_editing.is_some() {
            "Enter:save  Esc:cancel"
        } else {
            "j/k:navigate  Enter:activate  d:delete  ,:back"
        };
        let status = ui::build_status("SET", app.core.focus, hints, chunks[1].width);
        frame.render_widget(
            Paragraph::new(status).style(
                Style::default()
                    .fg(theme::status_fg())
                    .bg(theme::status_bg()),
            ),
            chunks[1],
        );
        return;
    }

    if app.mode == AppMode::Explore {
        app.core.content_area = chunks[0];
        crate::explore::draw_explore(frame, app, chunks[0]);

        let status = build_explore_status(app, chunks[1].width);
        frame.render_widget(
            Paragraph::new(status).style(
                Style::default()
                    .fg(theme::status_fg())
                    .bg(theme::status_bg()),
            ),
            chunks[1],
        );

        if app.core.help_visible {
            let area = ui::centered_rect(50, 60, frame.area());
            ui::draw_help(frame, area, &azide_help_sections());
        }
        return;
    }

    if app.core.sidebar_visible {
        let body_chunks =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(chunks[0]);

        app.core.sidebar_area = body_chunks[0];
        app.core.content_area = body_chunks[1];

        let sidebar_border_style = if app.core.focus == Focus::Sidebar {
            Style::default().fg(theme::border_focused())
        } else {
            Style::default().fg(theme::border_unfocused())
        };
        let content_border_style = if app.core.focus == Focus::Content {
            Style::default().fg(theme::border_focused())
        } else {
            Style::default().fg(theme::border_unfocused())
        };

        crate::ui::draw_sidebar(frame, app, body_chunks[0], sidebar_border_style);
        draw_content_area(frame, app, body_chunks[1], content_border_style);
    } else {
        app.core.sidebar_area = ratatui::layout::Rect::default();
        app.core.content_area = chunks[0];

        let content_border_style = Style::default().fg(theme::border_focused());
        draw_content_area(frame, app, chunks[0], content_border_style);
    }

    // Compute article line count for scroll indicator (needs content area width)
    if app.rss_view == RssView::ArticleContent
        && let Some(feed) = app.feed_store.feeds.get(app.feed_index)
        && let Some(article) = feed.articles.get(app.article_index)
    {
        let content_width = app.core.content_area.width.saturating_sub(2) as usize;
        let header = if article.categories.is_empty() { 4 } else { 5 };
        let content = jones_render::html::render_html(&article.content);
        let mut visual_lines = header;
        for line in &content.lines {
            let line_width: usize = line.spans.iter().map(|s| s.content.len()).sum();
            visual_lines += if content_width > 0 && line_width > content_width {
                line_width.div_ceil(content_width)
            } else {
                1
            };
        }
        app.article_line_count = visual_lines;
    }

    let hints = build_hints(app);
    let status = ui::build_status("RSS", app.core.focus, &hints, chunks[1].width);
    frame.render_widget(
        Paragraph::new(status).style(
            Style::default()
                .fg(theme::status_fg())
                .bg(theme::status_bg()),
        ),
        chunks[1],
    );

    if app.core.searching {
        let area = ui::centered_rect(50, 40, frame.area());
        let results: Vec<(usize, String)> = app
            .core
            .search_results
            .iter()
            .map(|&idx| {
                (
                    idx,
                    app.feed_store
                        .feeds
                        .get(idx)
                        .map(|f| f.title.clone())
                        .unwrap_or_default(),
                )
            })
            .collect();
        ui::draw_search(
            frame,
            &app.core.search_query,
            &results,
            app.core.search_index,
            area,
        );
    }

    if app.core.help_visible {
        let area = ui::centered_rect(50, 60, frame.area());
        ui::draw_help(frame, area, &azide_help_sections());
    }

    if app.creating_view {
        let area = ui::centered_rect(50, 50, frame.area());
        crate::ui::draw_create_view_modal(frame, app, area);
    }
}

fn draw_content_area(
    frame: &mut Frame,
    app: &mut AzideApp,
    area: ratatui::layout::Rect,
    border_style: Style,
) {
    if app.rss_view == RssView::ViewTimeline {
        crate::ui::draw_view_timeline(frame, app, area, border_style);
    } else {
        crate::ui::draw_content(frame, app, area, border_style);
    }
}

fn build_hints(app: &AzideApp) -> String {
    let mut hints = String::new();

    // Context-sensitive hints
    match app.core.focus {
        Focus::Sidebar => match app.rss_view {
            RssView::FeedList => {
                hints.push_str("a:add  v:view  d:del  M:all-read  r:ref  ");
            }
            RssView::ViewTimeline => {
                hints.push_str("Enter:sel  Esc:back  ");
            }
            RssView::ArticleList => {
                hints.push_str("Enter:open  m:mark  M:all-read  s:star  o:link  r:ref  Esc:back  ");
            }
            RssView::ArticleContent => {
                hints.push_str("m:mark  s:star  Esc:back  ");
            }
        },
        Focus::Content => {
            if app.rss_view == RssView::ViewTimeline {
                hints.push_str("Enter:expand  m:mark  s:star  o:link  Esc:back  ");
            } else {
                hints.push_str("j/k:scroll  m:mark  s:star  o:link  Esc:back  ");
            }
        }
    }

    // Global hints
    hints.push_str("b:bar  ,:set  /:find  ?:help");

    // Refresh indicator
    if app.refreshing {
        hints.push_str("  [Refreshing...]");
    }

    // Scroll position indicator for article reading
    if app.core.focus == Focus::Content && app.rss_view == RssView::ArticleContent {
        let visible = app.core.content_area.height.saturating_sub(2) as usize;
        let pos = if app.article_line_count == 0 || app.article_scroll == 0 {
            "Top".to_string()
        } else {
            let scrollable = app.article_line_count.saturating_sub(visible);
            if scrollable == 0 || app.article_scroll as usize >= scrollable {
                "End".to_string()
            } else {
                format!("{}%", (app.article_scroll as usize * 100) / scrollable)
            }
        };
        hints.push_str(&format!("  [{pos}]"));
    }
    hints
}

fn build_explore_status(app: &AzideApp, width: u16) -> Line<'static> {
    let pane = match app.explore_focus {
        ExploreFocus::CategoryList => "categories",
        ExploreFocus::FeedList => "feeds",
    };
    let badge = " EXP ".to_string();
    let prefix = format!(" {pane} | ");
    let hints = build_explore_hints(app.explore_focus);
    let reserved = badge.chars().count() + prefix.chars().count() + 1;
    let hints_budget = (width as usize).saturating_sub(reserved);
    let display_hints = truncate_hint_sections(&hints, hints_budget);

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

fn build_explore_hints(focus: ExploreFocus) -> String {
    let mut sections = vec![
        "Tab:switch pane",
        "j/k/↑↓:move",
        "Click:select",
        "Esc:back",
        "?:help",
    ];

    if focus == ExploreFocus::FeedList {
        sections.insert(2, "Enter:subscribe");
    }

    sections.join("  ")
}

fn truncate_hint_sections(hints: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }

    if hints.chars().count() <= budget {
        return hints.to_string();
    }

    if budget <= 1 {
        return String::new();
    }

    let mut result = String::new();
    let mut used = 0usize;

    for section in hints.split("  ") {
        let separator = usize::from(!result.is_empty()) * 2;
        let section_len = section.chars().count();
        if used + separator + section_len + 1 > budget {
            break;
        }
        if !result.is_empty() {
            result.push_str("  ");
            used += 2;
        }
        result.push_str(section);
        used += section_len;
    }

    if result.is_empty() {
        hints
            .chars()
            .take(budget.saturating_sub(1))
            .collect::<String>()
            + "…"
    } else {
        result.push('…');
        result
    }
}

fn azide_help_sections() -> Vec<ui::HelpSection> {
    vec![
        ui::HelpSection {
            title: "Global".to_string(),
            entries: vec![
                ("  q        ".to_string(), "Quit".to_string()),
                (
                    "  Tab      ".to_string(),
                    "Toggle sidebar / content focus".to_string(),
                ),
                (
                    "  b        ".to_string(),
                    "Toggle sidebar visibility".to_string(),
                ),
                ("  ,        ".to_string(), "Open settings".to_string()),
                ("  /        ".to_string(), "Search".to_string()),
                ("  ?        ".to_string(), "Toggle this help".to_string()),
            ],
        },
        ui::HelpSection {
            title: "Navigation".to_string(),
            entries: vec![
                (
                    "  j / Down ".to_string(),
                    "Move down / scroll down".to_string(),
                ),
                ("  k / Up   ".to_string(), "Move up / scroll up".to_string()),
                ("  Enter    ".to_string(), "Open / select".to_string()),
                (
                    "  Esc      ".to_string(),
                    "Go back / close overlay".to_string(),
                ),
                ("  Space    ".to_string(), "Page down".to_string()),
                ("  g        ".to_string(), "Scroll to top".to_string()),
            ],
        },
        ui::HelpSection {
            title: "RSS".to_string(),
            entries: vec![
                ("  a        ".to_string(), "Add feed by URL".to_string()),
                (
                    "  d        ".to_string(),
                    "Delete selected feed".to_string(),
                ),
                ("  r        ".to_string(), "Refresh all feeds".to_string()),
                (
                    "  m        ".to_string(),
                    "Toggle read / unread".to_string(),
                ),
                (
                    "  M        ".to_string(),
                    "Mark all read in feed".to_string(),
                ),
                (
                    "  s / *    ".to_string(),
                    "Toggle star / save for later".to_string(),
                ),
                (
                    "  o        ".to_string(),
                    "Open article link in browser".to_string(),
                ),
            ],
        },
        ui::HelpSection {
            title: "Views".to_string(),
            entries: vec![
                ("  v        ".to_string(), "Create new view".to_string()),
                (
                    "  Enter    ".to_string(),
                    "Open view / expand article".to_string(),
                ),
                (
                    "  Esc      ".to_string(),
                    "Collapse article / exit view".to_string(),
                ),
                (
                    "  o        ".to_string(),
                    "Open article link in browser".to_string(),
                ),
                (
                    "  d        ".to_string(),
                    "Delete view (when selected)".to_string(),
                ),
            ],
        },
        ui::HelpSection {
            title: "Explore".to_string(),
            entries: vec![
                ("  e        ".to_string(), "Open explore view".to_string()),
                (
                    "  Tab      ".to_string(),
                    "Switch between Categories and Feeds".to_string(),
                ),
                (
                    "  j/k / ↑↓ ".to_string(),
                    "Move selection in the focused pane".to_string(),
                ),
                (
                    "  Enter    ".to_string(),
                    "Subscribe to selected feed".to_string(),
                ),
                (
                    "  Click    ".to_string(),
                    "Select category or feed and focus that pane".to_string(),
                ),
                (
                    "  Wheel    ".to_string(),
                    "Move selection in hovered or focused pane".to_string(),
                ),
                ("  Esc / q  ".to_string(), "Close explore".to_string()),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_hint_sections_handles_multibyte_boundary() {
        let hints = "Tab:switch pane  j/k/↑↓:move  Click:select";
        assert_eq!(truncate_hint_sections(hints, 23), "Tab:switch pane…");
        assert_eq!(truncate_hint_sections(hints, 24), "Tab:switch pane…");
        assert_eq!(truncate_hint_sections(hints, 25), "Tab:switch pane…");
    }

    #[test]
    fn explore_hints_are_context_sensitive() {
        assert!(!build_explore_hints(ExploreFocus::CategoryList).contains("Enter:subscribe"));
        assert!(build_explore_hints(ExploreFocus::FeedList).contains("Enter:subscribe"));
    }
}
