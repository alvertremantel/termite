use crate::app::{AzideApp, ExploreFocus};
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VisibleFeedItem {
    pub index: usize,
    pub start_row: usize,
    pub height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisibleFeedWindow {
    pub start_index: usize,
    pub end_index: usize,
    pub items: Vec<VisibleFeedItem>,
}

/// Compute the category and feed pane `Rect`s inside the outer Explore
/// block area.  The outer `Rect` is the full area *before* the outer
/// `Borders::ALL` block border is subtracted; this helper accounts for the
/// border and applies the adaptive split (38/62 for inner width < 80,
/// 30/70 otherwise).
///
/// Returns `None` when the outer area is too small to hold borders + panes.
pub(crate) fn explore_pane_layout(outer_area: Rect) -> Option<(Rect, Rect)> {
    if outer_area.width <= 2 || outer_area.height <= 2 {
        return None;
    }
    let inner = Rect {
        x: outer_area.x.saturating_add(1),
        y: outer_area.y.saturating_add(1),
        width: outer_area.width.saturating_sub(2),
        height: outer_area.height.saturating_sub(2),
    };
    let (cat_pct, feed_pct) = if inner.width < 80 { (38, 62) } else { (30, 70) };
    let chunks = Layout::horizontal([
        Constraint::Percentage(cat_pct),
        Constraint::Percentage(feed_pct),
    ])
    .split(inner);
    Some((chunks[0], chunks[1]))
}

pub fn draw_explore(frame: &mut Frame, app: &AzideApp, area: Rect) {
    let block = Block::default()
        .title(" Explore ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_focused()))
        .style(Style::default().bg(theme::bg_surface()));

    frame.render_widget(block, area);

    let Some((cat_area, feed_area)) = explore_pane_layout(area) else {
        return;
    };

    draw_categories(frame, app, cat_area);
    draw_feeds(frame, app, feed_area);
}

fn draw_categories(frame: &mut Frame, app: &AzideApp, area: Rect) {
    let is_focused = app.explore_focus == ExploreFocus::CategoryList;
    let border_style = if is_focused {
        Style::default().fg(theme::border_focused())
    } else {
        Style::default().fg(theme::border_unfocused())
    };

    let count = app.explore_categories.len();
    let title = format!(" Categories ({count}) ");
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.explore_categories.is_empty() {
        let empty =
            Paragraph::new("No categories loaded.\n\nCheck that explore defaults\nare available.")
                .style(Style::default().fg(theme::text_muted()));
        frame.render_widget(empty, inner);
        return;
    }

    let text_width = inner.width.saturating_sub(2) as usize;

    let visible = category_visible_window(
        app.explore_categories.len(),
        app.explore_category_index,
        inner.height as usize,
    );

    let items: Vec<ListItem> = app
        .explore_categories
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.end.saturating_sub(visible.start))
        .map(|(i, cat)| {
            let style = if i == app.explore_category_index && is_focused {
                Style::default()
                    .fg(theme::rss_accent())
                    .add_modifier(Modifier::BOLD)
            } else if i == app.explore_category_index {
                Style::default()
                    .fg(theme::text_bright())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let prefix = if i == app.explore_category_index {
                "> "
            } else {
                "  "
            };

            // Append feed count inline: "> Name (N)"
            let suffix = format!(" ({})", cat.feeds.len());
            let avail = text_width.saturating_sub(suffix.len());
            let name = truncate_str(&cat.name, avail);
            let line_text = format!("{prefix}{name}{suffix}");

            ListItem::new(Line::from(Span::styled(line_text, style)))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(
        app.explore_category_index.saturating_sub(visible.start),
    ));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(theme::accent_blue_bright())
                .bg(theme::bg_highlight())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn draw_feeds(frame: &mut Frame, app: &AzideApp, area: Rect) {
    let is_focused = app.explore_focus == ExploreFocus::FeedList;
    let border_style = if is_focused {
        Style::default().fg(theme::border_focused())
    } else {
        Style::default().fg(theme::border_unfocused())
    };

    let feeds = app
        .explore_categories
        .get(app.explore_category_index)
        .map(|c| c.feeds.as_slice())
        .unwrap_or(&[]);

    let cat_name = app
        .explore_categories
        .get(app.explore_category_index)
        .map(|c| c.name.as_str())
        .unwrap_or("");

    let title = if cat_name.is_empty() {
        " Feeds ".to_string()
    } else {
        format!(" {} ({}) ", cat_name, feeds.len())
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if feeds.is_empty() {
        let hint = if app.explore_categories.is_empty() {
            "No categories loaded."
        } else {
            "No feeds in this category.\nUse j/k and Tab to browse."
        };
        let empty = Paragraph::new(hint).style(Style::default().fg(theme::text_muted()));
        frame.render_widget(empty, inner);
        return;
    }

    let text_width = inner.width.saturating_sub(4) as usize; // "> " prefix + margin

    let visible = feed_visible_window(feeds, app.explore_feed_index, inner.height as usize);

    let items: Vec<ListItem> = feeds
        .iter()
        .enumerate()
        .skip(visible.start_index)
        .take(visible.end_index.saturating_sub(visible.start_index))
        .map(|(i, feed)| {
            let selected = i == app.explore_feed_index;
            let title_style = if selected && is_focused {
                Style::default()
                    .fg(theme::rss_accent())
                    .add_modifier(Modifier::BOLD)
            } else if selected {
                Style::default()
                    .fg(theme::text_bright())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let prefix = if selected { "> " } else { "  " };

            let mut lines: Vec<Line> = Vec::new();

            // Primary: feed title
            let title_text = truncate_str(&feed.title, text_width);
            lines.push(Line::from(Span::styled(
                format!("{prefix}{title_text}"),
                title_style,
            )));

            // Secondary: description (dimmed, when available)
            if !feed.description.is_empty() {
                let desc = truncate_str(&feed.description, text_width);
                lines.push(Line::from(Span::styled(
                    format!("  {desc}"),
                    Style::default().fg(theme::text_dim()),
                )));
            }

            // Metadata: URL (muted — secondary to the title)
            if !feed.url.is_empty() {
                let url = truncate_str(&feed.url, text_width);
                lines.push(Line::from(Span::styled(
                    format!("  {url}"),
                    Style::default().fg(theme::text_muted()),
                )));
            }

            ListItem::new(Text::from(lines))
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(
        app.explore_feed_index.saturating_sub(visible.start_index),
    ));

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(theme::accent_blue_bright())
                .bg(theme::bg_highlight())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, inner, &mut list_state);
}

/// Truncate a string to fit within a maximum character count, appending `…`
/// when truncation occurs. Correctly counts Unicode characters, not bytes.
///
/// - Returns the original string unchanged when `s.chars().count() <= max_width`.
/// - When `max_width == 1`, returns just `"…"` (no room for any original chars).
/// - When `max_width == 0`, returns an empty string.
fn truncate_str(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let char_count = s.chars().count();
    if char_count <= max_width {
        return s.to_string();
    }
    // Reserve one character for the ellipsis.
    let take = max_width.saturating_sub(1);
    let truncated: String = s.chars().take(take).collect();
    format!("{}…", truncated)
}

pub(crate) fn category_visible_window(
    item_count: usize,
    selected: usize,
    viewport_height: usize,
) -> std::ops::Range<usize> {
    if item_count == 0 || viewport_height == 0 {
        return 0..0;
    }

    let selected = selected.min(item_count - 1);
    let start = selected.saturating_add(1).saturating_sub(viewport_height);
    let end = start.saturating_add(viewport_height).min(item_count);
    start..end
}

pub(crate) fn visible_category_index_at_row(
    item_count: usize,
    selected: usize,
    viewport_height: usize,
    row: usize,
) -> Option<usize> {
    let visible = category_visible_window(item_count, selected, viewport_height);
    let index = visible.start.saturating_add(row);
    (index < visible.end).then_some(index)
}

pub(crate) fn feed_item_height(feed: &azide_explore::FeedDefault) -> usize {
    1 + usize::from(!feed.description.is_empty()) + usize::from(!feed.url.is_empty())
}

pub(crate) fn feed_visible_window(
    feeds: &[azide_explore::FeedDefault],
    selected: usize,
    viewport_height: usize,
) -> VisibleFeedWindow {
    if feeds.is_empty() || viewport_height == 0 {
        return VisibleFeedWindow {
            start_index: 0,
            end_index: 0,
            items: Vec::new(),
        };
    }

    let selected = selected.min(feeds.len() - 1);
    let mut start_index = 0;
    let mut window_height = 0usize;

    for (index, feed) in feeds.iter().enumerate().take(selected + 1) {
        window_height += feed_item_height(feed);
        while start_index < index && window_height > viewport_height {
            window_height = window_height.saturating_sub(feed_item_height(&feeds[start_index]));
            start_index += 1;
        }
    }

    let mut items = Vec::new();
    let mut row = 0usize;
    let mut end_index = start_index;

    for (index, feed) in feeds.iter().enumerate().skip(start_index) {
        let height = feed_item_height(feed);
        if !items.is_empty() && row + height > viewport_height {
            break;
        }

        items.push(VisibleFeedItem {
            index,
            start_row: row,
            height,
        });
        row += height;
        end_index = index + 1;

        if row >= viewport_height {
            break;
        }
    }

    VisibleFeedWindow {
        start_index,
        end_index,
        items,
    }
}

pub(crate) fn visible_feed_index_at_row(
    feeds: &[azide_explore::FeedDefault],
    selected: usize,
    viewport_height: usize,
    row: usize,
) -> Option<usize> {
    let visible = feed_visible_window(feeds, selected, viewport_height);
    visible
        .items
        .into_iter()
        .find(|item| row >= item.start_row && row < item.start_row + item.height)
        .map(|item| item.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(title: &str, description: &str, url: &str) -> azide_explore::FeedDefault {
        azide_explore::FeedDefault {
            title: title.to_string(),
            description: description.to_string(),
            url: url.to_string(),
        }
    }

    // ── truncate_str ────────────────────────────────────────────────

    #[test]
    fn truncate_exact_fit_no_ellipsis() {
        assert_eq!(truncate_str("hello", 5), "hello");
        assert_eq!(truncate_str("hi", 2), "hi");
    }

    #[test]
    fn truncate_fits_with_room() {
        assert_eq!(truncate_str("hi", 10), "hi");
    }

    #[test]
    fn truncate_zero_width() {
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn truncate_ascii_short() {
        assert_eq!(truncate_str("hello world", 6), "hello…");
        assert_eq!(truncate_str("abc", 2), "a…");
    }

    #[test]
    fn truncate_single_char_budget() {
        assert_eq!(truncate_str("test", 1), "…");
        assert_eq!(truncate_str("x", 1), "x"); // exact fit
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate_str("", 5), "");
        assert_eq!(truncate_str("", 0), "");
    }

    #[test]
    fn truncate_multi_byte_unicode() {
        // 日本語 = 3 chars, 9 bytes
        assert_eq!(truncate_str("日本語", 3), "日本語");
        assert_eq!(truncate_str("日本語", 2), "日…");
        assert_eq!(truncate_str("日本語", 1), "…");
    }

    #[test]
    fn truncate_mixed_ascii_and_unicode() {
        // "café résumé" = 12 chars (including spaces)
        let s = "café résumé";
        assert_eq!(truncate_str(s, 12), s); // exact
        assert_eq!(truncate_str(s, 7), "café r…");
        assert_eq!(truncate_str(s, 4), "caf…");
    }

    #[test]
    fn truncate_emoji() {
        // 😀 is 1 char, 4 bytes
        assert_eq!(truncate_str("😀😀😀", 2), "😀…");
        assert_eq!(truncate_str("😀😀😀", 3), "😀😀😀");
        assert_eq!(truncate_str("😀", 1), "😀");
    }

    // ── explore_pane_layout ─────────────────────────────────────────

    #[test]
    fn pane_layout_wide_terminal() {
        // 120 cols → should use 30/70 split
        let outer = Rect::new(0, 0, 120, 30);
        let (cat, feed) = explore_pane_layout(outer).expect("should produce layout");
        // Outer 120 − 2 (border) = 118 inner width
        // 30% of 118 = floor(118 * 30 / 100) = 35.4 → ratatui uses integer math
        // ratatui percentage calculation: width * percentage / 100
        // 118 * 30 / 100 = 35, 118 * 70 / 100 = 82; remaining 1 goes to last constraint
        // So feed gets 83 (118 - 35)
        assert!(
            cat.width > 0 && cat.width < feed.width,
            "wide: cat({}) should be narrower than feed({})",
            cat.width,
            feed.width
        );
        // Category should be ~30% of inner width, not 38%
        let inner_width = outer.width - 2;
        assert_eq!(cat.x, outer.x + 1);
        assert_eq!(cat.y, outer.y + 1);
        assert_eq!(cat.height, outer.height - 2);
        assert_eq!(feed.height, outer.height - 2);
        assert!(
            cat.width < inner_width / 2,
            "wide: cat width {} > half inner {}",
            cat.width,
            inner_width
        );
    }

    #[test]
    fn pane_layout_narrow_terminal() {
        // 60 cols → should use 38/62 split
        let outer = Rect::new(0, 0, 60, 20);
        let (cat, feed) = explore_pane_layout(outer).expect("should produce layout");
        // Inner width 58; 38% of 58 ≈ 22
        assert!(
            cat.width > 0 && cat.width < feed.width,
            "narrow: cat({}) should be narrower than feed({})",
            cat.width,
            feed.width
        );
        // Verify border is inside outer
        assert_eq!(cat.x, outer.x + 1);
        assert_eq!(cat.y, outer.y + 1);
    }

    #[test]
    fn pane_layout_too_small_area() {
        assert!(explore_pane_layout(Rect::new(0, 0, 2, 10)).is_none());
        assert!(explore_pane_layout(Rect::new(0, 0, 10, 2)).is_none());
        assert!(explore_pane_layout(Rect::new(0, 0, 0, 10)).is_none());
    }

    #[test]
    fn pane_layout_exactly_at_threshold() {
        // inner.width = 80 → outer.width = 82. Should use wide split (30/70)?
        // Current logic: inner.width < 80 → narrow (38/62), else wide (30/70).
        // So at inner width 80, it's the "wide" path.
        let outer = Rect::new(0, 0, 82, 20); // inner.width = 80
        let (cat, _feed) = explore_pane_layout(outer).expect("should produce layout");
        let inner_width = outer.width - 2; // 80
        assert_eq!(inner_width, 80);
        // Wide split: cat ≈ 30% of 80 = 24
        assert!(
            cat.width < inner_width / 2,
            "at threshold: cat {} should be < half inner {}",
            cat.width,
            inner_width
        );

        // One less: inner.width = 79 → outer.width = 81 → narrow split
        let outer2 = Rect::new(0, 0, 81, 20);
        let (cat2, _feed2) = explore_pane_layout(outer2).expect("should produce layout");
        let inner_width2 = outer2.width - 2; // 79
        assert_eq!(inner_width2, 79);
        // Narrow split: cat ≈ 38% of 79 = 30
        assert!(cat2.width > inner_width2 * 30 / 100); // should be closer to 38%
    }

    #[test]
    fn category_visible_window_tracks_selected_row() {
        assert_eq!(category_visible_window(10, 0, 4), 0..4);
        assert_eq!(category_visible_window(10, 3, 4), 0..4);
        assert_eq!(category_visible_window(10, 4, 4), 1..5);
        assert_eq!(category_visible_window(10, 9, 4), 6..10);
    }

    #[test]
    fn visible_category_hit_test_uses_window_offset() {
        assert_eq!(visible_category_index_at_row(10, 6, 4, 0), Some(3));
        assert_eq!(visible_category_index_at_row(10, 6, 4, 3), Some(6));
        assert_eq!(visible_category_index_at_row(10, 6, 4, 4), None);
    }

    #[test]
    fn feed_visible_window_scrolls_to_selected_item() {
        let feeds = vec![
            feed("A", "", ""),
            feed("B", "desc", ""),
            feed("C", "desc", ""),
            feed("D", "", ""),
        ];

        let visible = feed_visible_window(&feeds, 2, 4);
        assert_eq!(visible.start_index, 1);
        assert_eq!(visible.end_index, 3);
        assert_eq!(
            visible.items,
            vec![
                VisibleFeedItem {
                    index: 1,
                    start_row: 0,
                    height: 2,
                },
                VisibleFeedItem {
                    index: 2,
                    start_row: 2,
                    height: 2,
                },
            ]
        );
    }

    #[test]
    fn visible_feed_hit_test_uses_rendered_window_rows() {
        let feeds = vec![
            feed("A", "", ""),
            feed("B", "desc", ""),
            feed("C", "desc", ""),
            feed("D", "", ""),
        ];

        assert_eq!(visible_feed_index_at_row(&feeds, 2, 4, 0), Some(1));
        assert_eq!(visible_feed_index_at_row(&feeds, 2, 4, 1), Some(1));
        assert_eq!(visible_feed_index_at_row(&feeds, 2, 4, 2), Some(2));
        assert_eq!(visible_feed_index_at_row(&feeds, 2, 4, 3), Some(2));
        assert_eq!(visible_feed_index_at_row(&feeds, 2, 4, 4), None);
    }
}
