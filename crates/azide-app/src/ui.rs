use crate::app::{AzideApp, RssView, SidebarItem};
use chrono::{DateTime, Local, Utc};
use jones_render::html;
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

pub fn draw_sidebar(frame: &mut Frame, app: &mut AzideApp, area: Rect, border_style: Style) {
    match app.rss_view {
        RssView::FeedList => {
            draw_feed_list(frame, app, area, border_style);
        }
        RssView::ArticleList | RssView::ArticleContent => {
            draw_article_list(frame, app, area, border_style);
        }
        RssView::ViewTimeline => {
            draw_feed_list(frame, app, area, border_style);
        }
    }
}

fn draw_feed_list(frame: &mut Frame, app: &mut AzideApp, area: Rect, border_style: Style) {
    use ratatui::layout::{Constraint, Layout};

    let block = Block::default()
        .title(" Feeds ")
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::bg_surface()));

    if app.adding_feed {
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);

        draw_sidebar_items(frame, app, chunks[0]);

        let input_line = Line::from(vec![
            Span::styled("URL> ", Style::default().fg(theme::search_prompt())),
            Span::raw(app.add_feed_input.clone()),
            Span::styled("_", Style::default().fg(theme::search_prompt())),
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[1]);
        return;
    }

    if app.feed_store.feeds.is_empty() && app.core.config.rss.views.is_empty() {
        let help = Paragraph::new(vec![
            Line::from(""),
            Line::from("  No feeds yet."),
            Line::from(""),
            Line::from("  Press 'a' to add a feed URL"),
            Line::from("  or add feeds in config.toml"),
        ])
        .block(block)
        .style(Style::default().fg(theme::text_muted()));
        frame.render_widget(help, area);
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_sidebar_items(frame, app, inner);
}

fn draw_sidebar_items(frame: &mut Frame, app: &mut AzideApp, area: Rect) {
    let items: Vec<ListItem> = app
        .sidebar_items
        .iter()
        .map(|item| match item {
            SidebarItem::ViewHeader => ListItem::new(Line::from(Span::styled(
                "── Views ──────",
                Style::default().fg(theme::section_header()),
            ))),
            SidebarItem::View(i) => {
                if let Some(view) = app.core.config.rss.views.get(*i) {
                    let unread: usize = view
                        .feeds
                        .iter()
                        .filter_map(|url| app.feed_store.feeds.iter().find(|f| &f.url == url))
                        .map(|f| f.unread_count)
                        .sum();
                    let count_str = if unread > 0 {
                        format!(" ({})", unread)
                    } else {
                        String::new()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {}", view.name),
                            Style::default().fg(theme::text_primary()),
                        ),
                        Span::styled(count_str, Style::default().fg(theme::unread_count())),
                    ]))
                } else {
                    ListItem::new(Line::from("  ???"))
                }
            }
            SidebarItem::FeedHeader => ListItem::new(Line::from(Span::styled(
                "── Feeds ──────",
                Style::default().fg(theme::section_header()),
            ))),
            SidebarItem::Feed(i) => {
                if let Some(feed) = app.feed_store.feeds.get(*i) {
                    let unread = if feed.unread_count > 0 {
                        format!(" ({})", feed.unread_count)
                    } else {
                        String::new()
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            feed.title.clone(),
                            Style::default().fg(theme::text_primary()),
                        ),
                        Span::styled(unread, Style::default().fg(theme::unread_count())),
                    ]))
                } else {
                    ListItem::new(Line::from("???"))
                }
            }
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(theme::accent_blue_bright())
                .bg(theme::bg_highlight())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.feed_list_state);
}

fn draw_article_list(frame: &mut Frame, app: &mut AzideApp, area: Rect, border_style: Style) {
    let block = Block::default()
        .title(" Articles ")
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::bg_surface()));

    let feed = match app.feed_store.feeds.get(app.feed_index) {
        Some(f) => f,
        None => {
            frame.render_widget(Paragraph::new("No feed selected").block(block), area);
            return;
        }
    };

    let items: Vec<ListItem> = feed
        .articles
        .iter()
        .map(|article| {
            let style = if article.read {
                Style::default().fg(theme::read_article())
            } else {
                Style::default().fg(theme::unread_article())
            };

            let star = if article.starred { " *" } else { "" };
            let date = format_published(article.published);
            // Pad date to fixed width so titles align (inbox-style)
            let date_col = format!("{:<14}", date);
            ListItem::new(Line::from(vec![
                Span::styled(date_col, Style::default().fg(theme::text_secondary())),
                Span::styled(article.title.clone(), style),
                Span::styled(star.to_string(), Style::default().fg(theme::star())),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(theme::accent_blue_bright())
                .bg(theme::bg_highlight())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.article_list_state);
}

pub fn draw_content(frame: &mut Frame, app: &AzideApp, area: Rect, border_style: Style) {
    let block = Block::default()
        .title(" Content ")
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::bg_surface()));

    let article = app
        .feed_store
        .feeds
        .get(app.feed_index)
        .and_then(|f| f.articles.get(app.article_index));

    match article {
        Some(article) => {
            let title_line = Line::from(Span::styled(
                article.title.clone(),
                Style::default()
                    .fg(theme::accent_blue_bright())
                    .add_modifier(Modifier::BOLD),
            ));

            // Byline: author . date . [Link]
            let mut byline_spans: Vec<Span> = Vec::new();
            if !article.author.is_empty() {
                byline_spans.push(Span::styled(
                    article.author.clone(),
                    Style::default().fg(theme::text_secondary()),
                ));
                byline_spans.push(Span::styled(
                    " · ",
                    Style::default().fg(theme::text_secondary()),
                ));
            }
            byline_spans.push(Span::styled(
                format_published(article.published),
                Style::default().fg(theme::text_secondary()),
            ));
            if !article.link.is_empty() {
                byline_spans.push(Span::styled(
                    " · ",
                    Style::default().fg(theme::text_secondary()),
                ));
                byline_spans.push(Span::styled(
                    "[Link]",
                    Style::default()
                        .fg(theme::link())
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            let byline = Line::from(byline_spans);

            let mut all_lines = vec![title_line, byline];

            if !article.categories.is_empty() {
                all_lines.push(Line::from(vec![
                    Span::styled("Tags: ", Style::default().fg(theme::text_secondary())),
                    Span::styled(
                        article.categories.clone(),
                        Style::default().fg(theme::text_secondary()),
                    ),
                ]));
            }

            let separator = Line::from(Span::styled(
                "\u{2500}".repeat(area.width.saturating_sub(2) as usize),
                Style::default().fg(theme::section_divider()),
            ));
            all_lines.push(separator);
            all_lines.push(Line::from(""));

            let mut content_text = html::render_html(&article.content);
            all_lines.append(&mut content_text.lines);

            let paragraph = Paragraph::new(all_lines)
                .block(block)
                .wrap(Wrap { trim: false })
                .scroll((app.article_scroll, 0));

            frame.render_widget(paragraph, area);
        }
        None => {
            let help = Paragraph::new("Select a feed and article to read.")
                .block(block)
                .style(Style::default().fg(theme::text_muted()));
            frame.render_widget(help, area);
        }
    }
}

fn format_published(timestamp: i64) -> String {
    if timestamp == 0 {
        return String::new();
    }
    let Some(dt) = DateTime::from_timestamp(timestamp, 0) else {
        return String::new();
    };
    let local: DateTime<Local> = dt.with_timezone(&Local);
    let now = Utc::now();
    let elapsed = now.signed_duration_since(dt);

    if elapsed.num_minutes() < 1 {
        "just now".to_string()
    } else if elapsed.num_hours() < 1 {
        format!("{}m ago", elapsed.num_minutes())
    } else if elapsed.num_hours() < 24 {
        format!("{}h ago", elapsed.num_hours())
    } else {
        let time = format_time(&local);
        if local.format("%Y").to_string() == now.format("%Y").to_string() {
            format!("{} {}", local.format("%b %d"), time)
        } else {
            format!("{} {}", local.format("%b %d '%y"), time)
        }
    }
}

fn format_time(dt: &DateTime<Local>) -> String {
    let hour = dt.format("%-I").to_string();
    let min = dt.format("%M").to_string();
    let ampm = if dt.format("%p").to_string() == "AM" {
        "a"
    } else {
        "p"
    };
    format!("{hour}:{min}{ampm}")
}

/// Calculate the number of rendered lines for an expanded article in ViewTimeline.
/// Used by both the renderer and the click/scroll handlers for consistency.
pub fn expanded_article_lines(article: &azide_store::ViewArticle, content_width: usize) -> usize {
    let content_text = html::render_html(&article.content);
    let wrap_width = content_width.saturating_sub(2);
    let mut content_lines = 0usize;
    for line in content_text.lines {
        content_lines += wrap_line(line, wrap_width).len();
    }

    let mut count = 1  // title line (collapsed row)
        + 1  // border top
        + 1  // inner title
        + 1  // byline
        + 1  // inner separator
        + content_lines
        + 1  // border bottom
        + 1; // blank line

    if !article.categories.is_empty() {
        count += 1;
    }

    count
}

pub fn draw_view_timeline(frame: &mut Frame, app: &AzideApp, area: Rect, border_style: Style) {
    let view_name = app
        .core
        .config
        .rss
        .views
        .get(app.view_index)
        .map(|v| v.name.as_str())
        .unwrap_or("View");

    let block = Block::default()
        .title(format!(" {view_name} "))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.view_articles.is_empty() {
        let help = Paragraph::new("No articles in this view.")
            .style(Style::default().fg(theme::text_muted()));
        frame.render_widget(help, inner);
        return;
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let content_width = inner.width.saturating_sub(4) as usize;

    for (i, article) in app.view_articles.iter().enumerate() {
        let is_selected = i == app.view_selected;
        let is_expanded = app.view_expanded == Some(i);

        // Title line
        let date = format_published(article.published);
        let date_col = format!("{:<14}", date);
        let tag = format!("[{}]", article.feed_tag);
        let tag_col = format!("{:<6}", tag);

        let title_style = if is_selected {
            Style::default()
                .fg(theme::rss_accent())
                .add_modifier(Modifier::BOLD)
        } else if article.read {
            Style::default().fg(theme::read_article())
        } else {
            Style::default().fg(theme::unread_article())
        };

        let prefix = if is_expanded {
            "\u{25bc} "
        } else if is_selected {
            "> "
        } else {
            "  "
        };

        lines.push(Line::from(vec![
            Span::raw(prefix.to_string()),
            Span::styled(date_col, Style::default().fg(theme::text_secondary())),
            Span::styled(tag_col, Style::default().fg(theme::accent_yellow_dim())),
            Span::styled(article.title.clone(), title_style),
        ]));

        if is_expanded {
            let border = "\u{2500}".repeat(content_width);
            let pipe = Span::styled("  \u{2502} ", Style::default().fg(theme::accent_blue_dim()));

            // Border top
            lines.push(Line::from(Span::styled(
                format!("  \u{250c}{border}\u{2510}"),
                Style::default().fg(theme::accent_blue_dim()),
            )));

            // Title (big, bold)
            lines.push(Line::from(vec![
                pipe.clone(),
                Span::styled(
                    article.title.clone(),
                    Style::default()
                        .fg(theme::article_title())
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Byline: author . date . [Link]
            let mut byline_spans = vec![pipe.clone()];
            if !article.author.is_empty() {
                byline_spans.push(Span::styled(
                    article.author.clone(),
                    Style::default().fg(theme::text_secondary()),
                ));
                byline_spans.push(Span::styled(
                    " \u{00b7} ",
                    Style::default().fg(theme::text_secondary()),
                ));
            }
            byline_spans.push(Span::styled(
                format_published(article.published),
                Style::default().fg(theme::text_secondary()),
            ));
            if !article.link.is_empty() {
                byline_spans.push(Span::styled(
                    " \u{00b7} ",
                    Style::default().fg(theme::text_secondary()),
                ));
                byline_spans.push(Span::styled(
                    "[Link]",
                    Style::default()
                        .fg(theme::link())
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            lines.push(Line::from(byline_spans));

            // Categories/tags
            if !article.categories.is_empty() {
                lines.push(Line::from(vec![
                    pipe.clone(),
                    Span::styled("Tags: ", Style::default().fg(theme::text_secondary())),
                    Span::styled(
                        article.categories.clone(),
                        Style::default().fg(theme::text_secondary()),
                    ),
                ]));
            }

            // Separator inside box
            lines.push(Line::from(Span::styled(
                format!(
                    "  \u{2502} {}",
                    "\u{2500}".repeat(content_width.saturating_sub(2))
                ),
                Style::default().fg(theme::section_divider()),
            )));

            // Content -- wrapped to content_width
            let content_text = html::render_html(&article.content);
            for content_line in content_text.lines {
                let wrapped = wrap_line(content_line, content_width.saturating_sub(2));
                for wl in wrapped {
                    let mut spans = vec![pipe.clone()];
                    spans.extend(wl.spans);
                    lines.push(Line::from(spans));
                }
            }

            // Border bottom
            lines.push(Line::from(Span::styled(
                format!("  \u{2514}{border}\u{2518}"),
                Style::default().fg(theme::accent_blue_dim()),
            )));

            lines.push(Line::from(""));
        }
    }

    let paragraph = Paragraph::new(lines).scroll((app.view_scroll, 0));

    frame.render_widget(paragraph, inner);
}

pub fn draw_create_view_modal(frame: &mut Frame, app: &AzideApp, area: Rect) {
    use ratatui::widgets::Clear;

    frame.render_widget(Clear, area);

    match app.create_view_step {
        0 => {
            // Name input
            let block = Block::default()
                .title(" Create View ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent_blue()))
                .style(Style::default().bg(theme::bg_surface()));

            let inner = block.inner(area);
            frame.render_widget(block, area);

            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Name: ", Style::default().fg(theme::search_prompt())),
                    Span::raw(app.create_view_name.clone()),
                    Span::styled("_", Style::default().fg(theme::search_prompt())),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  Enter: next  Esc: cancel",
                    Style::default().fg(theme::unfocused()),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        1 => {
            // Feed checklist
            let title = format!(" Create View: \"{}\" ", app.create_view_name.trim());
            let block = Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent_blue()))
                .style(Style::default().bg(theme::bg_surface()));

            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Reserve space for header and footer hints
            let header_lines = 2; // blank + "Select feeds:"
            let footer_lines = 2; // blank + hints
            let list_height = inner.height.saturating_sub(header_lines + footer_lines) as usize;

            // Scroll the cursor into view
            let scroll_offset = if app.create_view_cursor >= list_height {
                app.create_view_cursor - list_height + 1
            } else {
                0
            };

            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                "  Select feeds:",
                Style::default().fg(theme::unfocused()),
            )));
            lines.push(Line::from(""));

            let visible_end = (scroll_offset + list_height).min(app.feed_store.feeds.len());
            for i in scroll_offset..visible_end {
                let feed = &app.feed_store.feeds[i];
                let checked = app.create_view_feeds.get(i).copied().unwrap_or(false);
                let marker = if checked { "[x] " } else { "[ ] " };
                let is_cursor = i == app.create_view_cursor;

                let style = if is_cursor {
                    Style::default()
                        .fg(theme::rss_accent())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let prefix = if is_cursor { "> " } else { "  " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(marker, Style::default().fg(theme::accent_blue())),
                    Span::styled(feed.title.clone(), style),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Space: toggle feed  Enter: save view  Esc: back",
                Style::default().fg(theme::unfocused()),
            )));

            frame.render_widget(Paragraph::new(lines), inner);
        }
        _ => {}
    }
}

/// Wrap a Line into multiple lines respecting word boundaries.
/// Words are never split mid-character unless they exceed max_width entirely.
fn wrap_line(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![line];
    }

    let total_width: usize = line.spans.iter().map(|s| s.content.len()).sum();
    if total_width <= max_width {
        return vec![line];
    }

    let mut result: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for span in line.spans {
        let style = span.style;
        let content = span.content.into_owned();

        if current_width + content.len() <= max_width {
            current_width += content.len();
            current_spans.push(Span::styled(content, style));
            continue;
        }

        // Need to split this span across lines
        let mut remaining = content.as_str();
        while !remaining.is_empty() {
            let available = max_width.saturating_sub(current_width);

            if available == 0 {
                result.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
                remaining = remaining.trim_start();
                continue;
            }

            if remaining.len() <= available {
                current_spans.push(Span::styled(remaining.to_string(), style));
                current_width += remaining.len();
                break;
            }

            // Check if the available window ends at a word boundary
            if remaining.as_bytes()[available] == b' ' {
                let chunk = &remaining[..available];
                current_spans.push(Span::styled(chunk.to_string(), style));
                result.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
                remaining = remaining[available + 1..].trim_start();
                continue;
            }

            // Find a word boundary to break at
            match remaining[..available].rfind(' ') {
                Some(pos) => {
                    // Take content up to the space
                    let chunk = &remaining[..pos];
                    if !chunk.is_empty() {
                        current_spans.push(Span::styled(chunk.to_string(), style));
                    }
                    // Flush line, skip the space, trim leading whitespace
                    result.push(Line::from(std::mem::take(&mut current_spans)));
                    current_width = 0;
                    remaining = remaining[pos + 1..].trim_start();
                }
                None => {
                    if current_width > 0 {
                        // No space found but line has content — move word to next line
                        result.push(Line::from(std::mem::take(&mut current_spans)));
                        current_width = 0;
                    } else {
                        // Word is longer than max_width; force-split
                        let (chunk, rest) = remaining.split_at(available);
                        current_spans.push(Span::styled(chunk.to_string(), style));
                        result.push(Line::from(std::mem::take(&mut current_spans)));
                        current_width = 0;
                        remaining = rest;
                    }
                }
            }
        }
    }

    if !current_spans.is_empty() {
        result.push(Line::from(current_spans));
    }

    if result.is_empty() {
        result.push(Line::from(""));
    }

    result
}
