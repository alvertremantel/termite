use azide_config::Config;
use crossterm::event::{KeyCode, KeyEvent};
use jones_theme as theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::AzideApp;

// ── Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Reader,
    Settings,
    Explore,
}

#[derive(Debug, Clone)]
pub enum SettingsItem {
    Blank,
    SectionHeader(String),
    Feed {
        id: i64,
        title: String,
        url: String,
    },
    AddFeed,
    View {
        index: usize,
        name: String,
        feed_count: usize,
    },
    CreateView,
    Theme {
        current: String,
        name: String,
        description: String,
    },
    InfoLine(String),
    BackupDatabase,
}

impl SettingsItem {
    pub fn is_selectable(&self) -> bool {
        matches!(
            self,
            Self::Feed { .. }
                | Self::AddFeed
                | Self::View { .. }
                | Self::CreateView
                | Self::Theme { .. }
                | Self::BackupDatabase
        )
    }
}

#[derive(Debug)]
pub enum SettingsEdit {
    FeedUrl { feed_id: i64, input: String },
    NewFeedUrl { input: String },
}

/// Actions the settings handler asks the app to perform.
pub enum SettingsAction {
    None,
    ExitSettings,
    AddFeed(String),
    DeleteFeed(i64),
    DeleteView(usize),
    SaveFeedUrl(i64, String),
    OpenCreateViewModal,
    BackupDatabase,
    CycleTheme,
}

// ── Build items ──────────────────────────────────────────────────────

pub fn rebuild_items(app: &AzideApp) -> Vec<SettingsItem> {
    let mut items = Vec::new();

    // Feeds section
    items.push(SettingsItem::SectionHeader("Feeds".to_string()));
    for feed in &app.feed_store.feeds {
        items.push(SettingsItem::Feed {
            id: feed.id,
            title: feed.title.clone(),
            url: feed.url.clone(),
        });
    }
    items.push(SettingsItem::AddFeed);

    // Views section
    items.push(SettingsItem::Blank);
    items.push(SettingsItem::SectionHeader("Views".to_string()));
    for (i, view) in app.core.config.rss.views.iter().enumerate() {
        items.push(SettingsItem::View {
            index: i,
            name: view.name.clone(),
            feed_count: view.feeds.len(),
        });
    }
    items.push(SettingsItem::CreateView);

    // UI section
    items.push(SettingsItem::Blank);
    items.push(SettingsItem::SectionHeader("UI".to_string()));
    let active_theme = theme::find(&app.core.config.ui.theme).unwrap_or_else(theme::current);
    items.push(SettingsItem::Theme {
        current: active_theme.id.to_string(),
        name: active_theme.name.to_string(),
        description: active_theme.description.to_string(),
    });

    // Data section
    items.push(SettingsItem::Blank);
    items.push(SettingsItem::SectionHeader("Data".to_string()));
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("azide/config.toml")
        .display()
        .to_string();
    let db_path = Config::data_dir().join("azide.db").display().to_string();
    items.push(SettingsItem::InfoLine(format!("  Config:   {config_path}")));
    items.push(SettingsItem::InfoLine(format!("  Database: {db_path}")));
    items.push(SettingsItem::BackupDatabase);

    items
}

// ── Navigation ───────────────────────────────────────────────────────

pub fn first_selectable(items: &[SettingsItem]) -> usize {
    items
        .iter()
        .position(|item| item.is_selectable())
        .unwrap_or(0)
}

fn next_selectable(items: &[SettingsItem], from: usize) -> usize {
    for (i, item) in items.iter().enumerate().skip(from + 1) {
        if item.is_selectable() {
            return i;
        }
    }
    from
}

fn prev_selectable(items: &[SettingsItem], from: usize) -> usize {
    if from == 0 {
        return from;
    }
    for i in (0..from).rev() {
        if items[i].is_selectable() {
            return i;
        }
    }
    from
}

pub fn auto_scroll(index: usize, scroll: u16, visible_height: u16) -> u16 {
    let line = index as u16;
    if line < scroll {
        line
    } else if visible_height > 0 && line >= scroll + visible_height {
        line.saturating_sub(visible_height) + 1
    } else {
        scroll
    }
}

// ── Drawing ──────────────────────────────────────────────────────────

pub fn draw_settings(frame: &mut Frame, app: &AzideApp, area: Rect) {
    let block = Block::default()
        .title(" Settings [Esc: back] ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_focused()))
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    for (i, item) in app.settings_items.iter().enumerate() {
        let is_selected = i == app.settings_index;

        // Handle inline editing states
        if is_selected && let Some(ref edit) = app.settings_editing {
            match (edit, item) {
                (SettingsEdit::FeedUrl { feed_id, input }, SettingsItem::Feed { id, .. })
                    if *id == *feed_id =>
                {
                    lines.push(editing_line(input));
                    continue;
                }
                (SettingsEdit::NewFeedUrl { input }, SettingsItem::AddFeed) => {
                    lines.push(editing_line(input));
                    continue;
                }
                _ => {}
            }
        }

        lines.push(render_item(item, is_selected));
    }

    let paragraph = Paragraph::new(lines).scroll((app.settings_scroll, 0));

    frame.render_widget(paragraph, inner);
}

fn editing_line(input: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("  URL> ", Style::default().fg(theme::accent_yellow())),
        Span::styled(input.to_string(), Style::default().fg(theme::text_bright())),
        Span::styled("_", Style::default().fg(theme::accent_yellow())),
    ])
}

fn render_item(item: &SettingsItem, selected: bool) -> Line<'static> {
    match item {
        SettingsItem::Blank => Line::from(""),

        SettingsItem::SectionHeader(name) => {
            let pad = 50usize.saturating_sub(name.len() + 4);
            Line::from(Span::styled(
                format!("── {name} {}", "─".repeat(pad)),
                Style::default().fg(theme::section_header()),
            ))
        }

        SettingsItem::Feed { title, url, .. } => {
            let prefix = if selected { "▸ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme::accent_blue_bright())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let url_style = Style::default().fg(if selected {
                theme::text_secondary()
            } else {
                theme::text_muted()
            });
            let hint = if selected { "  Enter:edit d:del" } else { "" };

            let display_title = if title.is_empty() || title == url {
                "(untitled)"
            } else {
                title.as_str()
            };
            let truncated_url = if url.len() > 40 {
                let end: String = url.chars().take(37).collect();
                format!("{end}...")
            } else {
                url.clone()
            };

            let mut spans = vec![
                Span::styled(prefix.to_string(), style),
                Span::styled(display_title.to_string(), style),
                Span::styled(format!("  ({truncated_url})"), url_style),
            ];
            if !hint.is_empty() {
                spans.push(Span::styled(
                    hint.to_string(),
                    Style::default().fg(theme::text_muted()),
                ));
            }
            Line::from(spans).style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }

        SettingsItem::AddFeed => {
            let style = if selected {
                Style::default()
                    .fg(theme::accent_yellow())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::accent_yellow_dim())
            };
            Line::from(Span::styled("  [+ Add Feed]", style)).style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }

        SettingsItem::View {
            name, feed_count, ..
        } => {
            let prefix = if selected { "▸ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme::accent_blue_bright())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let count_style = Style::default().fg(if selected {
                theme::text_secondary()
            } else {
                theme::text_muted()
            });
            let hint = if selected { "  d:del" } else { "" };
            Line::from(vec![
                Span::styled(prefix.to_string(), style),
                Span::styled(name.clone(), style),
                Span::styled(format!("  ({feed_count} feeds)"), count_style),
                Span::styled(hint.to_string(), Style::default().fg(theme::text_muted())),
            ])
            .style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }

        SettingsItem::CreateView => {
            let style = if selected {
                Style::default()
                    .fg(theme::accent_yellow())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::accent_yellow_dim())
            };
            Line::from(Span::styled("  [+ Create View]", style)).style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }

        SettingsItem::Theme {
            current,
            name,
            description,
        } => {
            let prefix = if selected { "▸ " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(theme::accent_blue_bright())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text_primary())
            };
            let meta_style = Style::default().fg(if selected {
                theme::text_secondary()
            } else {
                theme::text_muted()
            });
            let hint = if selected { "  Enter:cycle" } else { "" };
            Line::from(vec![
                Span::styled(prefix.to_string(), style),
                Span::styled("Theme: ".to_string(), style),
                Span::styled(name.clone(), style),
                Span::styled(format!("  ({current}) — {description}"), meta_style),
                Span::styled(hint.to_string(), Style::default().fg(theme::text_muted())),
            ])
            .style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }

        SettingsItem::InfoLine(text) => Line::from(Span::styled(
            text.clone(),
            Style::default().fg(theme::text_muted()),
        )),

        SettingsItem::BackupDatabase => {
            let style = if selected {
                Style::default()
                    .fg(theme::accent_yellow())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::accent_yellow_dim())
            };
            Line::from(Span::styled("  [Backup Database]", style)).style(if selected {
                Style::default().bg(theme::bg_highlight())
            } else {
                Style::default()
            })
        }
    }
}

// ── Key handling ─────────────────────────────────────────────────────

pub fn handle_settings_key(app: &mut AzideApp, key: KeyEvent) -> SettingsAction {
    // Editing mode
    if let Some(ref mut edit) = app.settings_editing {
        match key.code {
            KeyCode::Esc => {
                app.settings_editing = None;
                return SettingsAction::None;
            }
            KeyCode::Enter => {
                let action = match edit {
                    SettingsEdit::FeedUrl { feed_id, input } => {
                        let url = input.trim().to_string();
                        if !url.is_empty() {
                            SettingsAction::SaveFeedUrl(*feed_id, url)
                        } else {
                            SettingsAction::None
                        }
                    }
                    SettingsEdit::NewFeedUrl { input } => {
                        let url = input.trim().to_string();
                        if !url.is_empty() {
                            SettingsAction::AddFeed(url)
                        } else {
                            SettingsAction::None
                        }
                    }
                };
                app.settings_editing = None;
                return action;
            }
            KeyCode::Backspace => {
                match edit {
                    SettingsEdit::FeedUrl { input, .. } | SettingsEdit::NewFeedUrl { input } => {
                        input.pop();
                    }
                }
                return SettingsAction::None;
            }
            KeyCode::Char(c) => {
                match edit {
                    SettingsEdit::FeedUrl { input, .. } | SettingsEdit::NewFeedUrl { input } => {
                        input.push(c);
                    }
                }
                return SettingsAction::None;
            }
            _ => return SettingsAction::None,
        }
    }

    // Normal mode
    match key.code {
        KeyCode::Esc | KeyCode::Char(',') => SettingsAction::ExitSettings,

        KeyCode::Down | KeyCode::Char('j') => {
            app.settings_index = next_selectable(&app.settings_items, app.settings_index);
            SettingsAction::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.settings_index = prev_selectable(&app.settings_items, app.settings_index);
            SettingsAction::None
        }

        KeyCode::Enter => match app.settings_items.get(app.settings_index).cloned() {
            Some(SettingsItem::Feed { id, url, .. }) => {
                app.settings_editing = Some(SettingsEdit::FeedUrl {
                    feed_id: id,
                    input: url,
                });
                SettingsAction::None
            }
            Some(SettingsItem::AddFeed) => {
                app.settings_editing = Some(SettingsEdit::NewFeedUrl {
                    input: String::new(),
                });
                SettingsAction::None
            }
            Some(SettingsItem::CreateView) => SettingsAction::OpenCreateViewModal,
            Some(SettingsItem::Theme { .. }) => SettingsAction::CycleTheme,
            Some(SettingsItem::BackupDatabase) => SettingsAction::BackupDatabase,
            _ => SettingsAction::None,
        },

        KeyCode::Char('d') => match app.settings_items.get(app.settings_index).cloned() {
            Some(SettingsItem::Feed { id, .. }) => SettingsAction::DeleteFeed(id),
            Some(SettingsItem::View { index, .. }) => SettingsAction::DeleteView(index),
            _ => SettingsAction::None,
        },

        _ => SettingsAction::None,
    }
}

// ── Mouse handling ───────────────────────────────────────────────────

pub fn handle_settings_mouse(app: &mut AzideApp, row: u16, area: Rect) {
    if app.settings_editing.is_some() {
        return;
    }

    // Map click row to item index (1:1 since each item is one line)
    let inner_y = area.y + 1; // account for border
    if row < inner_y {
        return;
    }
    let clicked_line = (row - inner_y) as usize + app.settings_scroll as usize;

    if clicked_line < app.settings_items.len() && app.settings_items[clicked_line].is_selectable() {
        app.settings_index = clicked_line;
    }
}

// ── Actions ──────────────────────────────────────────────────────────

pub fn backup_database() -> Result<String, String> {
    let db_path = Config::data_dir().join("azide.db");
    if !db_path.exists() {
        return Err("Database file not found.".to_string());
    }

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup_name = format!("azide-backup-{timestamp}.db");
    let backup_path = Config::data_dir().join(&backup_name);

    std::fs::copy(&db_path, &backup_path).map_err(|e| format!("Backup failed: {e}"))?;

    Ok(backup_path.display().to_string())
}
