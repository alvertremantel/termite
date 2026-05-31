use crate::model::{ArticleRow, GuiModel, Selection};
use azide_explore::Defaults;
use azide_store::Database;
use chrono::{Local, TimeZone};
use eframe::egui;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver};

pub struct AzideGui {
    db: Option<Database>,
    model: Option<GuiModel>,
    defaults: Defaults,
    add_feed_url: String,
    view_name: String,
    view_feed_ids: Vec<i64>,
    active_explore_category: usize,
    refresh: Option<Receiver<RefreshResult>>,
    article_text_cache: ArticleTextCache,
    date_cache: DateCache,
    error: Option<String>,
}

struct RefreshResult {
    status: Result<String, String>,
}

#[derive(Default)]
struct ArticleTextCache {
    entries: HashMap<i64, CachedArticleText>,
}

struct CachedArticleText {
    fingerprint: u64,
    preview: String,
    paragraphs: Vec<String>,
}

#[derive(Default)]
struct DateCache {
    entries: HashMap<i64, String>,
}

impl ArticleTextCache {
    fn get(&mut self, article_id: i64, content: &str) -> &CachedArticleText {
        let fingerprint = content_fingerprint(content);
        let entry = self
            .entries
            .entry(article_id)
            .or_insert_with(|| CachedArticleText::from_content(fingerprint, content));
        if entry.fingerprint != fingerprint {
            *entry = CachedArticleText::from_content(fingerprint, content);
        }
        entry
    }

    fn retain_articles(&mut self, article_ids: &HashSet<i64>) {
        self.entries
            .retain(|article_id, _| article_ids.contains(article_id));
    }
}

impl CachedArticleText {
    fn from_content(fingerprint: u64, content: &str) -> Self {
        let plain = plain_article_text(content);
        let preview = truncate(&plain, 180);
        let paragraphs = plain
            .split("\n\n")
            .map(str::trim)
            .filter(|paragraph| !paragraph.is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            fingerprint,
            preview,
            paragraphs,
        }
    }

    fn preview(&self) -> &str {
        &self.preview
    }

    fn paragraphs(&self) -> impl Iterator<Item = &str> {
        self.paragraphs.iter().map(String::as_str)
    }
}

impl DateCache {
    fn get(&mut self, timestamp: i64) -> &str {
        self.entries
            .entry(timestamp)
            .or_insert_with(|| format_timestamp(timestamp))
    }
}

enum ReaderAction {
    SetStarred(bool),
    SetRead(bool),
    Open(String),
    DeleteFeed(i64),
}

impl AzideGui {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

        let defaults = azide_explore::load_defaults();
        let (db, model, error) = match Database::open_default() {
            Ok(db) => match GuiModel::load(&db) {
                Ok(model) => (Some(db), Some(model), None),
                Err(error) => (Some(db), None, Some(error.to_string())),
            },
            Err(error) => (None, None, Some(error.to_string())),
        };

        Self {
            db,
            model,
            defaults,
            add_feed_url: String::new(),
            view_name: String::new(),
            view_feed_ids: Vec::new(),
            active_explore_category: 0,
            refresh: None,
            article_text_cache: ArticleTextCache::default(),
            date_cache: DateCache::default(),
            error,
        }
    }

    fn with_model(
        &mut self,
        action: impl FnOnce(&Database, &mut GuiModel) -> color_eyre::Result<()>,
    ) {
        let Some(db) = &self.db else {
            return;
        };
        let Some(model) = &mut self.model else {
            return;
        };
        let result = action(db, model);
        match result {
            Ok(()) => self.prune_article_cache(),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn prune_article_cache(&mut self) {
        let Some(model) = &self.model else {
            self.article_text_cache.entries.clear();
            return;
        };
        let mut article_ids = model
            .articles
            .iter()
            .map(|row| row.article.id)
            .collect::<HashSet<_>>();
        if let Some(row) = model.selected_article() {
            article_ids.insert(row.article.id);
        }
        self.article_text_cache.retain_articles(&article_ids);
    }

    fn start_refresh(&mut self, ctx: egui::Context) {
        if self.refresh.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.refresh = Some(rx);
        std::thread::spawn(move || {
            let status = match Database::open_default() {
                Ok(db) => match GuiModel::load(&db) {
                    Ok(mut model) => {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|error| error.to_string());
                        match runtime {
                            Ok(runtime) => runtime
                                .block_on(model.refresh_all(&db))
                                .map(|_| model.status)
                                .map_err(|error| error.to_string()),
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error.to_string()),
                },
                Err(error) => Err(error.to_string()),
            };
            let _ = tx.send(RefreshResult { status });
            ctx.request_repaint();
        });
    }

    fn receive_refresh(&mut self) {
        if let Some(rx) = &self.refresh {
            match rx.try_recv() {
                Ok(result) => {
                    match result.status {
                        Ok(status) => {
                            self.reload_current_model_after_refresh(status);
                        }
                        Err(error) => self.error = Some(error),
                    }
                    self.refresh = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Refresh worker disconnected".to_string());
                    self.refresh = None;
                }
            }
        }
    }

    fn reload_current_model_after_refresh(&mut self, status: String) {
        let Some(db) = &self.db else {
            return;
        };
        match self.model.as_mut() {
            Some(model) => match model.reload(db) {
                Ok(()) => {
                    model.status = status;
                    self.error = None;
                    self.prune_article_cache();
                }
                Err(error) => self.error = Some(error.to_string()),
            },
            None => match GuiModel::load(db) {
                Ok(mut model) => {
                    model.status = status;
                    self.model = Some(model);
                    self.error = None;
                    self.prune_article_cache();
                }
                Err(error) => self.error = Some(error.to_string()),
            },
        }
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("Azide");
            ui.separator();
            let refreshing = self.refresh.is_some();
            if ui
                .add_enabled(!refreshing, egui::Button::new("Refresh"))
                .clicked()
            {
                self.start_refresh(ctx.clone());
            }
            if refreshing {
                ui.spinner();
                ui.label("Refreshing feeds");
            }
            if let Some(model) = &self.model
                && !model.status.is_empty()
            {
                ui.separator();
                ui.label(&model.status);
            }
        });
    }

    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.model.as_ref() else {
            return;
        };

        let selection = model.selection;
        let mut next_selection = None;

        ui.heading("Library");
        ui.add_space(4.0);
        if sidebar_button(ui, "All Articles", selection == Selection::All).clicked() {
            next_selection = Some(Selection::All);
        }
        if sidebar_button(ui, "Unread", selection == Selection::Unread).clicked() {
            next_selection = Some(Selection::Unread);
        }
        if sidebar_button(ui, "Saved", selection == Selection::Saved).clicked() {
            next_selection = Some(Selection::Saved);
        }

        ui.add_space(14.0);
        ui.heading("Views");
        for (index, view) in model.config.rss.views.iter().enumerate() {
            let label = format!("{} ({})", view.name, view.feeds.len());
            if sidebar_button(ui, &label, selection == Selection::View(index)).clicked() {
                next_selection = Some(Selection::View(index));
            }
        }

        ui.add_space(14.0);
        ui.heading("Feeds");
        for feed in &model.feeds {
            let title = if feed.title.trim().is_empty() {
                feed.url.as_str()
            } else {
                feed.title.as_str()
            };
            let label = format!("{title}  {}", feed.unread_count);
            if sidebar_button(ui, &label, selection == Selection::Feed(feed.id)).clicked() {
                next_selection = Some(Selection::Feed(feed.id));
            }
        }

        if let Some(selection) = next_selection {
            self.with_model(|db, model| model.select(db, selection));
        }

        ui.separator();
        ui.label("Add feed");
        let response = ui.text_edit_singleline(&mut self.add_feed_url);
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.add_feed_from_input();
        }
        if ui.button("Add").clicked() {
            self.add_feed_from_input();
        }
    }

    fn add_feed_from_input(&mut self) {
        let url = self.add_feed_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        self.with_model(|db, model| {
            model.add_feed(db, &url)?;
            Ok(())
        });
        self.add_feed_url.clear();
    }

    fn draw_articles(&mut self, ui: &mut egui::Ui) {
        let (Some(model), article_text_cache, date_cache) = (
            self.model.as_ref(),
            &mut self.article_text_cache,
            &mut self.date_cache,
        ) else {
            return;
        };
        let rows = &model.articles;
        let selected = model.selected_article_id;
        let title = selection_title(model);
        let mut clicked_article_id = None;

        ui.horizontal(|ui| {
            ui.heading(title.as_ref());
            ui.separator();
            ui.label(format!("{} article(s)", rows.len()));
        });
        ui.separator();

        if rows.is_empty() {
            ui.add_space(20.0);
            ui.label("No articles here yet.");
            return;
        }

        egui::ScrollArea::vertical().show_rows(ui, 116.0, rows.len(), |ui, row_range| {
            for index in row_range {
                let row = &rows[index];
                let is_selected = selected == Some(row.article.id);
                let article_id = row.article.id;
                let article_text = article_text_cache.get(article_id, &row.article.content);
                let published =
                    (row.article.published > 0).then(|| date_cache.get(row.article.published));
                let response =
                    article_card(ui, row, article_text.preview(), published, is_selected);
                if response.clicked() {
                    clicked_article_id = Some(article_id);
                }
            }
        });

        if let Some(article_id) = clicked_article_id {
            self.with_model(|db, model| model.select_article(db, article_id));
        }
    }

    fn draw_reader(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (row, article_text, published) = {
            let (Some(model), article_text_cache, date_cache) = (
                self.model.as_ref(),
                &mut self.article_text_cache,
                &mut self.date_cache,
            ) else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select an article.");
                });
                return;
            };
            let Some(row) = model.selected_article() else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select an article.");
                });
                return;
            };
            (
                row,
                article_text_cache.get(row.article.id, &row.article.content),
                (row.article.published > 0).then(|| date_cache.get(row.article.published)),
            )
        };
        let mut action = None;

        ui.horizontal(|ui| {
            if ui
                .button(if row.article.starred {
                    "Unsave"
                } else {
                    "Save"
                })
                .clicked()
            {
                action = Some(ReaderAction::SetStarred(!row.article.starred));
            }
            if ui
                .button(if row.article.read {
                    "Mark unread"
                } else {
                    "Mark read"
                })
                .clicked()
            {
                action = Some(ReaderAction::SetRead(!row.article.read));
            }
            if !row.article.link.trim().is_empty() && ui.button("Open").clicked() {
                action = Some(ReaderAction::Open(row.article.link.clone()));
            }
            if ui.button("Delete feed").clicked() {
                action = Some(ReaderAction::DeleteFeed(row.feed_id));
            }
        });
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(&row.article.title);
            ui.horizontal_wrapped(|ui| {
                ui.label(&row.feed_title);
                if !row.article.author.is_empty() {
                    ui.separator();
                    ui.label(&row.article.author);
                }
                if let Some(published) = published {
                    ui.separator();
                    ui.label(published);
                }
            });
            if !row.article.categories.is_empty() {
                ui.label(&row.article.categories);
            }
            ui.add_space(10.0);
            for paragraph in article_text.paragraphs() {
                ui.label(egui::RichText::new(paragraph).size(15.0));
                ui.add_space(8.0);
            }
        });

        if let Some(action) = action {
            match action {
                ReaderAction::SetStarred(starred) => {
                    self.with_model(|db, model| model.set_selected_starred(db, starred));
                }
                ReaderAction::SetRead(read) => {
                    self.with_model(|db, model| model.set_selected_read(db, read));
                }
                ReaderAction::Open(link) => ctx.open_url(egui::OpenUrl::new_tab(link)),
                ReaderAction::DeleteFeed(feed_id) => {
                    self.with_model(|db, model| model.delete_feed(db, feed_id));
                }
            }
        }
    }

    fn draw_bottom(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.draw_view_editor(ui);
            ui.separator();
            self.draw_explore(ui);
            if let Some(error) = &self.error {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(220, 80, 70), error);
            }
        });
    }

    fn draw_view_editor(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label("View");
            ui.text_edit_singleline(&mut self.view_name);
            ui.horizontal_wrapped(|ui| {
                let Some(model) = self.model.as_ref() else {
                    return;
                };
                for feed in &model.feeds {
                    let mut selected = self.view_feed_ids.contains(&feed.id);
                    if ui
                        .checkbox(&mut selected, feed_title_text(&feed.title, &feed.url))
                        .changed()
                    {
                        if selected {
                            self.view_feed_ids.push(feed.id);
                        } else {
                            self.view_feed_ids.retain(|id| *id != feed.id);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Save view").clicked() {
                    let name = self.view_name.clone();
                    let ids = self.view_feed_ids.clone();
                    self.with_model(|db, model| model.save_view(db, &name, &ids));
                }
                if ui.button("Delete selected view").clicked()
                    && let Some(Selection::View(index)) = self.model.as_ref().map(|m| m.selection)
                {
                    self.with_model(|db, model| model.delete_view(db, index));
                }
            });
        });
    }

    fn draw_explore(&mut self, ui: &mut egui::Ui) {
        let mut add_feed = None;
        ui.vertical(|ui| {
            ui.label("Explore");
            egui::ComboBox::from_id_source("explore-category")
                .selected_text(
                    self.defaults
                        .category
                        .get(self.active_explore_category)
                        .map(|category| category.name.as_str())
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (index, category) in self.defaults.category.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.active_explore_category,
                            index,
                            &category.name,
                        );
                    }
                });
            if let Some(category) = self.defaults.category.get(self.active_explore_category) {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for feed in &category.feeds {
                            ui.group(|ui| {
                                ui.label(egui::RichText::new(&feed.title).strong());
                                ui.label(&feed.description);
                                if ui.button("Add").clicked() {
                                    add_feed = Some((feed.title.clone(), feed.url.clone()));
                                }
                            });
                        }
                    });
                });
            }
        });
        if let Some((title, url)) = add_feed {
            self.with_model(|db, model| model.add_explore_feed(db, &title, &url));
        }
    }
}

impl eframe::App for AzideGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_refresh();

        egui::TopBottomPanel::top("top").show(ctx, |ui| self.draw_top_bar(ui, ctx));
        egui::TopBottomPanel::bottom("bottom")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| self.draw_bottom(ui));
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(230.0)
            .width_range(190.0..=360.0)
            .show(ctx, |ui| self.draw_sidebar(ui));
        egui::SidePanel::right("reader")
            .resizable(true)
            .default_width(470.0)
            .width_range(360.0..=720.0)
            .show(ctx, |ui| self.draw_reader(ui, ctx));
        egui::CentralPanel::default().show(ctx, |ui| self.draw_articles(ui));
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(40, 120, 105);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 62, 74);
    visuals.selection.bg_fill = egui::Color32::from_rgb(32, 110, 98);
    ctx.set_visuals(visuals);
}

fn sidebar_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 26.0],
        egui::SelectableLabel::new(selected, text),
    )
}

fn article_card(
    ui: &mut egui::Ui,
    row: &ArticleRow,
    preview: &str,
    published: Option<&str>,
    selected: bool,
) -> egui::Response {
    let frame = egui::Frame::none()
        .fill(if selected {
            egui::Color32::from_rgb(35, 78, 74)
        } else {
            egui::Color32::from_rgb(28, 31, 35)
        })
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .rounding(egui::Rounding::same(6.0));
    frame
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if !row.article.read {
                    ui.colored_label(egui::Color32::from_rgb(100, 190, 170), "unread");
                }
                if row.article.starred {
                    ui.colored_label(egui::Color32::from_rgb(240, 190, 80), "saved");
                }
                ui.label(&row.feed_tag);
                if let Some(published) = published {
                    ui.label(published);
                }
            });
            ui.add(
                egui::Label::new(egui::RichText::new(&row.article.title).strong().size(16.0))
                    .truncate(),
            );
            if !preview.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(preview).color(egui::Color32::from_gray(185)),
                    )
                    .truncate(),
                );
            }
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn content_fingerprint(content: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn selection_title(model: &GuiModel) -> Cow<'_, str> {
    match model.selection {
        Selection::All => Cow::Borrowed("All Articles"),
        Selection::Unread => Cow::Borrowed("Unread"),
        Selection::Saved => Cow::Borrowed("Saved"),
        Selection::Feed(feed_id) => model
            .feeds
            .iter()
            .find(|feed| feed.id == feed_id)
            .map(|feed| feed_title_text(&feed.title, &feed.url))
            .unwrap_or(Cow::Borrowed("Feed")),
        Selection::View(index) => model
            .config
            .rss
            .views
            .get(index)
            .map(|view| Cow::Borrowed(view.name.as_str()))
            .unwrap_or(Cow::Borrowed("View")),
    }
}

fn feed_title_text<'a>(title: &'a str, url: &'a str) -> Cow<'a, str> {
    if title.trim().is_empty() {
        Cow::Borrowed(url)
    } else {
        Cow::Borrowed(title)
    }
}

fn format_timestamp(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%b %-d, %Y").to_string())
        .unwrap_or_else(|| "undated".to_string())
}

fn plain_article_text(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut tag = String::new();
    let mut in_tag = false;
    let mut skip = false;

    for ch in input.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag_name = tag
                    .trim()
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                match tag_name.as_str() {
                    "script" | "style" => skip = !tag.trim_start().starts_with('/'),
                    "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "blockquote"
                        if !result.ends_with('\n') =>
                    {
                        result.push('\n');
                    }
                    _ => {}
                }
            }
            _ if in_tag => tag.push(ch),
            _ if !skip => result.push(ch),
            _ => {}
        }
    }

    jones_render::html::decode_entities(&result)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    format!("{}...", input.chars().take(max_chars).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use azide_store::StoredArticle;
    use std::sync::Arc;

    fn article_row() -> ArticleRow {
        ArticleRow {
            feed_id: 1,
            feed_title: "Example".to_string(),
            feed_tag: "EXA".to_string(),
            article: Arc::new(StoredArticle {
                id: 10,
                guid: "guid-10".to_string(),
                title: "Readable title".to_string(),
                link: "https://example.test/article".to_string(),
                content: "Body".to_string(),
                published: 42,
                read: false,
                starred: false,
                author: String::new(),
                categories: String::new(),
            }),
        }
    }

    #[test]
    fn article_card_response_senses_clicks() {
        egui::__run_test_ui(|ui| {
            ui.set_width(320.0);
            let row = article_row();
            let response = article_card(ui, &row, "Preview text", Some("May 29, 2026"), false);

            assert!(response.sense.click);
            assert!(response.rect.width() >= 300.0);
        });
    }

    #[test]
    fn feed_title_text_falls_back_to_url() {
        assert_eq!(
            feed_title_text("", "https://example.test/rss").as_ref(),
            "https://example.test/rss"
        );
        assert_eq!(
            feed_title_text("Example", "https://example.test/rss").as_ref(),
            "Example"
        );
    }
}
