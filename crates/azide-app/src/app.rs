use crate::feed_store::{FeedStore, store_fetched_feeds};
use crate::settings::{self, AppMode, SettingsEdit};
use azide_config::{Config, ViewConfig};
use azide_feed::{fetch_all_feeds, generate_feed_tag};
use azide_store::{Database, ViewArticle};
use color_eyre::Result;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use jones_event as event;
use jones_event::{AppEvent, EventHandler};
use jones_search as search;
use jones_search::SearchAction;
use jones_state::{CoreState, Focus};
use jones_theme as theme;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RssView {
    FeedList,
    ArticleList,
    ArticleContent,
    ViewTimeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExploreFocus {
    CategoryList,
    FeedList,
}

#[derive(Debug, Clone)]
pub enum SidebarItem {
    ViewHeader,
    View(usize),
    FeedHeader,
    Feed(usize),
}

pub enum AzideCustom {
    FeedsRefreshed,
}

pub type AzideEvent = AppEvent<AzideCustom>;

pub struct AzideApp {
    pub core: CoreState<Config>,
    pub db: Database,
    pub feed_store: FeedStore,
    pub rss_view: RssView,
    pub feed_index: usize,
    pub article_index: usize,
    pub article_scroll: u16,
    pub feed_list_state: ListState,
    pub article_list_state: ListState,
    pub sidebar_items: Vec<SidebarItem>,
    pub sidebar_index: usize,
    pub view_articles: Vec<ViewArticle>,
    pub view_selected: usize,
    pub view_expanded: Option<usize>,
    pub view_scroll: u16,
    pub view_index: usize,
    pub creating_view: bool,
    pub create_view_step: u8,
    pub create_view_name: String,
    pub create_view_feeds: Vec<bool>,
    pub create_view_cursor: usize,
    pub adding_feed: bool,
    pub add_feed_input: String,
    pub article_line_count: usize,
    pub refreshing: bool,
    pub event_tx: Option<tokio::sync::mpsc::UnboundedSender<AzideEvent>>,
    pub mode: AppMode,
    pub settings_items: Vec<settings::SettingsItem>,
    pub settings_index: usize,
    pub settings_scroll: u16,
    pub settings_editing: Option<SettingsEdit>,
    pub explore_categories: Vec<azide_explore::Category>,
    pub explore_category_index: usize,
    pub explore_feed_index: usize,
    pub explore_focus: ExploreFocus,
}

impl AzideApp {
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;
        theme::set_current(&config.ui.theme);
        let db = Database::open_default()?;

        for url in &config.rss.feeds {
            let _ = db.add_feed(url, url);
        }

        let feed_store = FeedStore::new(&db)?;

        let mut feed_list_state = ListState::default();
        if !feed_store.feeds.is_empty() {
            feed_list_state.select(Some(0));
        }

        let mut app = Self {
            core: CoreState::new(config),
            db,
            feed_store,
            rss_view: RssView::FeedList,
            feed_index: 0,
            article_index: 0,
            article_scroll: 0,
            feed_list_state,
            article_list_state: ListState::default(),
            sidebar_items: Vec::new(),
            sidebar_index: 0,
            view_articles: Vec::new(),
            view_selected: 0,
            view_expanded: None,
            view_scroll: 0,
            view_index: 0,
            creating_view: false,
            create_view_step: 0,
            create_view_name: String::new(),
            create_view_feeds: Vec::new(),
            create_view_cursor: 0,
            adding_feed: false,
            add_feed_input: String::new(),
            article_line_count: 0,
            refreshing: false,
            event_tx: None,
            mode: AppMode::Reader,
            settings_items: Vec::new(),
            settings_index: 0,
            settings_scroll: 0,
            settings_editing: None,
            explore_categories: Vec::new(),
            explore_category_index: 0,
            explore_feed_index: 0,
            explore_focus: ExploreFocus::CategoryList,
        };
        let defaults = azide_explore::load_defaults();
        app.explore_categories = defaults.category;

        app.rebuild_sidebar_items();
        Ok(app)
    }

    pub fn rebuild_sidebar_items(&mut self) {
        self.sidebar_items.clear();
        if !self.core.config.rss.views.is_empty() {
            self.sidebar_items.push(SidebarItem::ViewHeader);
            for i in 0..self.core.config.rss.views.len() {
                self.sidebar_items.push(SidebarItem::View(i));
            }
        }
        if !self.core.config.rss.views.is_empty() {
            self.sidebar_items.push(SidebarItem::FeedHeader);
        }
        for i in 0..self.feed_store.feeds.len() {
            self.sidebar_items.push(SidebarItem::Feed(i));
        }
        self.sidebar_index = self
            .sidebar_items
            .iter()
            .position(|item| matches!(item, SidebarItem::View(_) | SidebarItem::Feed(_)))
            .unwrap_or(0);
        self.feed_list_state.select(Some(self.sidebar_index));
    }

    fn next_selectable_sidebar(&self, from: usize) -> usize {
        for i in (from + 1)..self.sidebar_items.len() {
            if matches!(
                self.sidebar_items[i],
                SidebarItem::View(_) | SidebarItem::Feed(_)
            ) {
                return i;
            }
        }
        from
    }

    fn prev_selectable_sidebar(&self, from: usize) -> usize {
        if from == 0 {
            return from;
        }
        for i in (0..from).rev() {
            if matches!(
                self.sidebar_items[i],
                SidebarItem::View(_) | SidebarItem::Feed(_)
            ) {
                return i;
            }
        }
        from
    }

    pub async fn run<B>(&mut self, terminal: &mut Terminal<B>) -> Result<()>
    where
        B: Backend,
        B::Error: Send + Sync + 'static,
    {
        let (mut events, event_tx) =
            EventHandler::<AzideCustom>::new(std::time::Duration::from_millis(100));
        self.event_tx = Some(event_tx.clone());

        if !self.feed_store.feeds.is_empty() {
            self.refreshing = true;
            let feed_urls: Vec<(i64, String)> = self
                .feed_store
                .feeds
                .iter()
                .map(|f| (f.id, f.url.clone()))
                .collect();
            let config = Config::load().ok();
            let tx = event_tx;
            tokio::spawn(async move {
                let results = fetch_all_feeds(&feed_urls).await;
                if config.is_some()
                    && let Ok(db) = Database::open_default()
                {
                    store_fetched_feeds(&db, &results);
                }
                let _ = tx.send(AppEvent::Custom(AzideCustom::FeedsRefreshed));
            });
        }

        while self.core.running {
            terminal.draw(|frame| crate::draw::draw(frame, self))?;
            if let Some(ev) = events.next().await {
                self.handle_event(ev).await;
            }
        }
        Ok(())
    }

    async fn handle_event(&mut self, ev: AzideEvent) {
        match ev {
            AppEvent::Key(key) => {
                if self.mode == AppMode::Settings {
                    // Ctrl+C always quits
                    if key.code == KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        self.core.running = false;
                        return;
                    }
                    // Create view modal takes priority when in settings
                    if self.creating_view {
                        self.handle_create_view_key(key);
                        return;
                    }
                    let action = settings::handle_settings_key(self, key);
                    self.handle_settings_action(action).await;
                    return;
                }
                if self.mode == AppMode::Explore {
                    if self.core.help_visible {
                        if key.code == KeyCode::Char('?') || key.code == KeyCode::Esc {
                            self.core.help_visible = false;
                        }
                        return;
                    }
                    self.handle_explore_key(key).await;
                    return;
                }
                if self.core.help_visible {
                    if key.code == KeyCode::Char('?') || key.code == KeyCode::Esc {
                        self.core.help_visible = false;
                    }
                    return;
                }
                if self.adding_feed {
                    self.handle_add_feed_key(key).await;
                    return;
                }
                if self.creating_view {
                    self.handle_create_view_key(key);
                    return;
                }
                if self.core.searching {
                    self.handle_search_key(key);
                    return;
                }
                if event::is_quit(&key) {
                    self.core.running = false;
                    return;
                }
                match key.code {
                    KeyCode::Tab => {
                        if !self.core.sidebar_visible {
                            self.core.sidebar_visible = true;
                            self.core.focus = Focus::Sidebar;
                        } else {
                            self.core.focus = match self.core.focus {
                                Focus::Sidebar => Focus::Content,
                                Focus::Content => Focus::Sidebar,
                            };
                        }
                    }
                    KeyCode::Char('b') => {
                        self.core.sidebar_visible = !self.core.sidebar_visible;
                        if !self.core.sidebar_visible {
                            self.core.focus = Focus::Content;
                        }
                    }
                    KeyCode::Char('/') => {
                        self.core.searching = true;
                        self.core.search_query.clear();
                        self.core.search_results.clear();
                        self.core.search_index = 0;
                    }
                    KeyCode::Char('?') => {
                        self.core.help_visible = true;
                    }
                    KeyCode::Char('e') => {
                        self.mode = AppMode::Explore;
                        self.explore_category_index = 0;
                        self.explore_feed_index = 0;
                        self.explore_focus = ExploreFocus::CategoryList;
                    }
                    KeyCode::Char(',') => {
                        self.mode = AppMode::Settings;
                        self.settings_items = settings::rebuild_items(self);
                        self.settings_index = settings::first_selectable(&self.settings_items);
                        self.settings_scroll = 0;
                        self.settings_editing = None;
                    }
                    _ => self.handle_rss_key(key).await,
                }
            }
            AppEvent::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            AppEvent::Tick => {}
            AppEvent::Resize(_, _) => {}
            AppEvent::Custom(AzideCustom::FeedsRefreshed) => {
                self.refreshing = false;
                if let Ok(store) = FeedStore::new(&self.db) {
                    self.feed_store = store;
                    self.rebuild_sidebar_items();
                    if !self.feed_store.feeds.is_empty() {
                        self.feed_index = self.feed_index.min(self.feed_store.feeds.len() - 1);
                        self.feed_list_state.select(Some(self.sidebar_index));
                        let article_len = self.feed_store.article_count(self.feed_index);
                        if article_len > 0 {
                            self.article_index = self.article_index.min(article_len - 1);
                            self.article_list_state.select(Some(self.article_index));
                        } else {
                            self.article_index = 0;
                            self.article_list_state.select(None);
                        }
                    } else {
                        self.feed_index = 0;
                        self.article_index = 0;
                        self.feed_list_state.select(None);
                        self.article_list_state.select(None);
                    }
                }
            }
        }
    }

    async fn handle_rss_key(&mut self, key: crossterm::event::KeyEvent) {
        match self.core.focus {
            Focus::Sidebar => match self.rss_view {
                RssView::FeedList => {
                    if event::is_nav_up(&key) {
                        self.sidebar_index = self.prev_selectable_sidebar(self.sidebar_index);
                        self.feed_list_state.select(Some(self.sidebar_index));
                    } else if event::is_nav_down(&key) {
                        self.sidebar_index = self.next_selectable_sidebar(self.sidebar_index);
                        self.feed_list_state.select(Some(self.sidebar_index));
                    } else if key.code == KeyCode::Enter {
                        match self.sidebar_items.get(self.sidebar_index).cloned() {
                            Some(SidebarItem::Feed(i)) => {
                                self.feed_index = i;
                                self.rss_view = RssView::ArticleList;
                                self.article_index = 0;
                                self.article_list_state.select(Some(0));
                            }
                            Some(SidebarItem::View(i)) => {
                                self.enter_view(i);
                            }
                            _ => {}
                        }
                    } else if key.code == KeyCode::Char('r') {
                        self.refresh_feeds_background();
                    } else if key.code == KeyCode::Char('M') {
                        // Mark all articles in selected feed as read
                        if let Some(SidebarItem::Feed(i)) =
                            self.sidebar_items.get(self.sidebar_index).cloned()
                        {
                            let prev = self.feed_index;
                            self.feed_index = i;
                            self.mark_all_read_in_feed();
                            self.feed_index = prev;
                        }
                    } else if key.code == KeyCode::Char('a') {
                        self.adding_feed = true;
                        self.add_feed_input.clear();
                    } else if key.code == KeyCode::Char('d') {
                        self.delete_sidebar_item();
                    } else if key.code == KeyCode::Char('v') {
                        self.creating_view = true;
                        self.create_view_step = 0;
                        self.create_view_name.clear();
                        self.create_view_feeds = vec![false; self.feed_store.feeds.len()];
                        self.create_view_cursor = 0;
                    }
                }
                RssView::ArticleList => {
                    if event::is_nav_up(&key) {
                        self.article_index = self.article_index.saturating_sub(1);
                        self.article_list_state.select(Some(self.article_index));
                    } else if event::is_nav_down(&key) {
                        let len = self.feed_store.article_count(self.feed_index);
                        if len > 0 {
                            self.article_index = (self.article_index + 1).min(len - 1);
                            self.article_list_state.select(Some(self.article_index));
                        }
                    } else if key.code == KeyCode::Enter {
                        self.rss_view = RssView::ArticleContent;
                        self.core.focus = Focus::Content;
                        self.article_scroll = 0;
                        self.feed_store
                            .mark_read(self.feed_index, self.article_index, &self.db);
                        self.article_line_count = 0; // computed in draw()
                    } else if key.code == KeyCode::Char('o') {
                        self.open_current_article_link();
                    } else if key.code == KeyCode::Char('m') {
                        // Toggle read/unread
                        self.toggle_article_read();
                    } else if key.code == KeyCode::Char('s') || key.code == KeyCode::Char('*') {
                        // Toggle star/save
                        self.toggle_article_star();
                    } else if key.code == KeyCode::Char('r') {
                        self.refresh_feeds_background();
                    } else if key.code == KeyCode::Char('M') {
                        self.mark_all_read_in_feed();
                    } else if key.code == KeyCode::Esc {
                        self.rss_view = RssView::FeedList;
                    }
                }
                RssView::ArticleContent => {
                    if key.code == KeyCode::Esc {
                        self.rss_view = RssView::ArticleList;
                    } else if key.code == KeyCode::Char('m') {
                        self.toggle_article_read();
                    } else if key.code == KeyCode::Char('s') || key.code == KeyCode::Char('*') {
                        self.toggle_article_star();
                    }
                }
                RssView::ViewTimeline => {
                    if event::is_nav_up(&key) {
                        self.sidebar_index = self.prev_selectable_sidebar(self.sidebar_index);
                        self.feed_list_state.select(Some(self.sidebar_index));
                    } else if event::is_nav_down(&key) {
                        self.sidebar_index = self.next_selectable_sidebar(self.sidebar_index);
                        self.feed_list_state.select(Some(self.sidebar_index));
                    } else if key.code == KeyCode::Enter {
                        match self.sidebar_items.get(self.sidebar_index).cloned() {
                            Some(SidebarItem::Feed(i)) => {
                                self.feed_index = i;
                                self.rss_view = RssView::ArticleList;
                                self.article_index = 0;
                                self.article_list_state.select(Some(0));
                            }
                            Some(SidebarItem::View(i)) => {
                                self.enter_view(i);
                            }
                            _ => {}
                        }
                    } else if key.code == KeyCode::Esc {
                        self.rss_view = RssView::FeedList;
                        self.view_articles.clear();
                        self.view_expanded = None;
                    }
                }
            },
            Focus::Content => {
                if self.rss_view == RssView::ViewTimeline {
                    if event::is_nav_up(&key) {
                        self.view_selected = self.view_selected.saturating_sub(1);
                        self.auto_scroll_view(self.core.content_area.height);
                    } else if event::is_nav_down(&key) {
                        if !self.view_articles.is_empty() {
                            self.view_selected =
                                (self.view_selected + 1).min(self.view_articles.len() - 1);
                            self.auto_scroll_view(self.core.content_area.height);
                        }
                    } else if key.code == KeyCode::Enter {
                        if self.view_expanded == Some(self.view_selected) {
                            self.view_expanded = None;
                        } else {
                            self.view_expanded = Some(self.view_selected);
                            if let Some(article) = self.view_articles.get_mut(self.view_selected)
                                && !article.read
                            {
                                article.read = true;
                                let _ = self.db.mark_read(article.id);
                                if let Some(feed) = self
                                    .feed_store
                                    .feeds
                                    .iter_mut()
                                    .find(|f| f.id == article.feed_id)
                                    && let Some(stored) =
                                        feed.articles.iter_mut().find(|a| a.id == article.id)
                                    && !stored.read
                                {
                                    stored.read = true;
                                    feed.unread_count = feed.unread_count.saturating_sub(1);
                                }
                            }
                        }
                        self.auto_scroll_view(self.core.content_area.height);
                    } else if key.code == KeyCode::Char('o') {
                        if let Some(article) = self.view_articles.get(self.view_selected)
                            && !article.link.is_empty()
                        {
                            let _ = open::that(&article.link);
                        }
                    } else if key.code == KeyCode::Char('m') {
                        // Toggle read/unread for selected view article
                        if let Some(article) = self.view_articles.get_mut(self.view_selected) {
                            article.read = !article.read;
                            let _ = self.db.set_read(article.id, article.read);
                            if let Some(feed) = self
                                .feed_store
                                .feeds
                                .iter_mut()
                                .find(|f| f.id == article.feed_id)
                                && let Some(stored) =
                                    feed.articles.iter_mut().find(|a| a.id == article.id)
                            {
                                stored.read = article.read;
                                if article.read {
                                    feed.unread_count = feed.unread_count.saturating_sub(1);
                                } else {
                                    feed.unread_count += 1;
                                }
                            }
                        }
                    } else if key.code == KeyCode::Char('s') || key.code == KeyCode::Char('*') {
                        // Toggle star for selected view article
                        if let Some(article) = self.view_articles.get_mut(self.view_selected) {
                            article.starred = !article.starred;
                            let _ = self.db.set_starred(article.id, article.starred);
                            if let Some(feed) = self
                                .feed_store
                                .feeds
                                .iter_mut()
                                .find(|f| f.id == article.feed_id)
                                && let Some(stored) =
                                    feed.articles.iter_mut().find(|a| a.id == article.id)
                            {
                                stored.starred = article.starred;
                            }
                        }
                    } else if key.code == KeyCode::Char(' ') {
                        self.view_scroll = self.view_scroll.saturating_add(20);
                    } else if key.code == KeyCode::Char('g') {
                        self.view_scroll = 0;
                    } else if key.code == KeyCode::Esc && self.core.sidebar_visible {
                        self.core.focus = Focus::Sidebar;
                    }
                } else {
                    if event::is_nav_up(&key) {
                        self.article_scroll = self.article_scroll.saturating_sub(1);
                    } else if event::is_nav_down(&key) {
                        self.article_scroll = self.article_scroll.saturating_add(1);
                    } else if key.code == KeyCode::Char(' ') {
                        self.article_scroll = self.article_scroll.saturating_add(20);
                    } else if key.code == KeyCode::Char('g') {
                        self.article_scroll = 0;
                    } else if key.code == KeyCode::Char('o') {
                        self.open_current_article_link();
                    } else if key.code == KeyCode::Char('m') {
                        self.toggle_article_read();
                    } else if key.code == KeyCode::Char('s') || key.code == KeyCode::Char('*') {
                        self.toggle_article_star();
                    } else if key.code == KeyCode::Esc {
                        if self.core.sidebar_visible {
                            self.core.focus = Focus::Sidebar;
                        }
                        self.rss_view = RssView::ArticleList;
                    }
                }
            }
        }
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        if self.mode == AppMode::Explore {
            if self.core.help_visible {
                return;
            }
            self.handle_explore_mouse(mouse);
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;
                if self.mode == AppMode::Settings {
                    settings::handle_settings_mouse(self, row, self.core.content_area);
                    return;
                }
                if col < self.core.sidebar_area.right() && row >= self.core.sidebar_area.y {
                    self.core.focus = Focus::Sidebar;
                    let clicked_row = (row - self.core.sidebar_area.y).saturating_sub(1) as usize;
                    match self.rss_view {
                        RssView::FeedList | RssView::ViewTimeline => {
                            if clicked_row < self.sidebar_items.len()
                                && matches!(
                                    self.sidebar_items[clicked_row],
                                    SidebarItem::View(_) | SidebarItem::Feed(_)
                                )
                            {
                                self.sidebar_index = clicked_row;
                                self.feed_list_state.select(Some(clicked_row));
                            }
                        }
                        RssView::ArticleList | RssView::ArticleContent => {
                            let len = self.feed_store.article_count(self.feed_index);
                            if clicked_row < len {
                                self.article_index = clicked_row;
                                self.article_list_state.select(Some(clicked_row));
                            }
                        }
                    }
                } else if col >= self.core.content_area.x && row >= self.core.content_area.y {
                    self.core.focus = Focus::Content;
                    // Click-to-select in ViewTimeline
                    if self.rss_view == RssView::ViewTimeline && !self.view_articles.is_empty() {
                        let clicked_line = (row - self.core.content_area.y).saturating_sub(1)
                            as usize
                            + self.view_scroll as usize;
                        let content_width = self.core.content_area.width.saturating_sub(6) as usize;
                        let mut line_offset = 0usize;
                        for (i, article) in self.view_articles.iter().enumerate() {
                            let item_lines = if self.view_expanded == Some(i) {
                                crate::ui::expanded_article_lines(article, content_width)
                            } else {
                                1
                            };
                            if clicked_line >= line_offset
                                && clicked_line < line_offset + item_lines
                            {
                                self.view_selected = i;
                                self.auto_scroll_view(self.core.content_area.height);
                                break;
                            }
                            line_offset += item_lines;
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.mode == AppMode::Settings {
                    self.settings_scroll = self.settings_scroll.saturating_sub(3);
                    return;
                }
                if self.core.focus == Focus::Content {
                    if self.rss_view == RssView::ViewTimeline {
                        self.view_scroll = self.view_scroll.saturating_sub(3);
                    } else {
                        self.article_scroll = self.article_scroll.saturating_sub(3);
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.mode == AppMode::Settings {
                    self.settings_scroll = self.settings_scroll.saturating_add(3);
                    return;
                }
                if self.core.focus == Focus::Content {
                    if self.rss_view == RssView::ViewTimeline {
                        self.view_scroll = self.view_scroll.saturating_add(3);
                    } else {
                        self.article_scroll = self.article_scroll.saturating_add(3);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_explore_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;

                if let Some((category_area, feed_area)) = self.explore_pane_areas() {
                    if let Some(index) = self.explore_category_hit_test(col, row, category_area) {
                        self.explore_focus = ExploreFocus::CategoryList;
                        self.explore_category_index = index;
                        self.explore_feed_index = 0;
                    } else if let Some(index) = self.explore_feed_hit_test(col, row, feed_area) {
                        self.explore_focus = ExploreFocus::FeedList;
                        self.explore_feed_index = index;
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                self.explore_scroll(-1, mouse.column, mouse.row);
            }
            MouseEventKind::ScrollDown => {
                self.explore_scroll(1, mouse.column, mouse.row);
            }
            _ => {}
        }
    }

    fn explore_pane_areas(&self) -> Option<(Rect, Rect)> {
        crate::explore::explore_pane_layout(self.core.content_area)
    }

    fn explore_category_hit_test(&self, col: u16, row: u16, area: Rect) -> Option<usize> {
        let inner = Self::inner_rect(area)?;
        if !Self::contains(inner, col, row) || self.explore_categories.is_empty() {
            return None;
        }

        crate::explore::visible_category_index_at_row(
            self.explore_categories.len(),
            self.explore_category_index,
            inner.height as usize,
            (row - inner.y) as usize,
        )
    }

    fn explore_feed_hit_test(&self, col: u16, row: u16, area: Rect) -> Option<usize> {
        let inner = Self::inner_rect(area)?;
        if !Self::contains(inner, col, row) {
            return None;
        }

        let category = self.explore_categories.get(self.explore_category_index)?;

        crate::explore::visible_feed_index_at_row(
            &category.feeds,
            self.explore_feed_index,
            inner.height as usize,
            (row - inner.y) as usize,
        )
    }

    fn explore_scroll(&mut self, delta: i32, col: u16, row: u16) {
        let hovered_focus = self
            .explore_pane_areas()
            .and_then(|(category_area, feed_area)| {
                if Self::inner_rect(category_area)
                    .is_some_and(|area| Self::contains(area, col, row))
                {
                    Some(ExploreFocus::CategoryList)
                } else if Self::inner_rect(feed_area)
                    .is_some_and(|area| Self::contains(area, col, row))
                {
                    Some(ExploreFocus::FeedList)
                } else {
                    None
                }
            });

        let target_focus = hovered_focus.unwrap_or(self.explore_focus);
        self.explore_focus = target_focus;

        match target_focus {
            ExploreFocus::CategoryList => {
                if self.explore_categories.is_empty() {
                    return;
                }
                let max_index = self.explore_categories.len() - 1;
                self.explore_category_index =
                    Self::step_index(self.explore_category_index, delta, max_index);
                self.explore_feed_index = 0;
            }
            ExploreFocus::FeedList => {
                let Some(category) = self.explore_categories.get(self.explore_category_index)
                else {
                    return;
                };
                if category.feeds.is_empty() {
                    return;
                }
                let max_index = category.feeds.len() - 1;
                self.explore_feed_index =
                    Self::step_index(self.explore_feed_index, delta, max_index);
            }
        }
    }

    fn inner_rect(area: Rect) -> Option<Rect> {
        (area.width > 2 && area.height > 2).then_some(Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        })
    }

    fn contains(area: Rect, col: u16, row: u16) -> bool {
        col >= area.x && col < area.right() && row >= area.y && row < area.bottom()
    }

    fn step_index(current: usize, delta: i32, max_index: usize) -> usize {
        if delta < 0 {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize).min(max_index)
        }
    }

    fn handle_search_key(&mut self, key: crossterm::event::KeyEvent) {
        let action = search::handle_search_key(&mut self.core, key);
        match action {
            SearchAction::Selected(idx) => {
                self.feed_index = idx;
                self.feed_list_state.select(Some(idx));
            }
            SearchAction::Updated => {
                self.core.search_results = search::update_search_results(
                    &self.core.search_query,
                    self.feed_store
                        .feeds
                        .iter()
                        .enumerate()
                        .map(|(i, f)| (i, f.title.as_str())),
                );
                self.core.search_index = 0;
            }
            _ => {}
        }
    }

    async fn handle_add_feed_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.adding_feed = false;
            }
            KeyCode::Enter => {
                let url = self.add_feed_input.trim().to_string();
                if !url.is_empty() {
                    let _ = self.feed_store.add_feed_url(&url, &self.db);
                    self.feed_store.refresh_all(&self.db).await;
                }
                self.adding_feed = false;
            }
            KeyCode::Backspace => {
                self.add_feed_input.pop();
            }
            KeyCode::Char(c) => {
                self.add_feed_input.push(c);
            }
            _ => {}
        }
    }

    fn delete_current_feed(&mut self) {
        if self.feed_store.feeds.is_empty() {
            return;
        }
        if let Some(feed) = self.feed_store.feeds.get(self.feed_index) {
            let _ = self.db.delete_feed(feed.id);
        }
        self.feed_store.feeds.remove(self.feed_index);
        if self.feed_index >= self.feed_store.feeds.len() && self.feed_index > 0 {
            self.feed_index -= 1;
        }
        self.feed_list_state.select(Some(self.feed_index));
    }

    fn enter_view(&mut self, view_index: usize) {
        self.view_index = view_index;
        if let Some(view) = self.core.config.rss.views.get(view_index) {
            let feed_id_tags: Vec<(i64, String)> = view
                .feeds
                .iter()
                .filter_map(|url| {
                    self.feed_store
                        .feeds
                        .iter()
                        .find(|f| &f.url == url)
                        .map(|f| (f.id, generate_feed_tag(&f.title)))
                })
                .collect();
            self.view_articles = self
                .db
                .get_articles_for_feeds(&feed_id_tags, 200)
                .unwrap_or_default();
            self.view_selected = 0;
            self.view_expanded = None;
            self.view_scroll = 0;
            self.rss_view = RssView::ViewTimeline;
            self.core.focus = Focus::Content;
        }
    }

    fn delete_sidebar_item(&mut self) {
        match self.sidebar_items.get(self.sidebar_index).cloned() {
            Some(SidebarItem::Feed(i)) => {
                self.feed_index = i;
                self.delete_current_feed();
                self.rebuild_sidebar_items();
            }
            Some(SidebarItem::View(i)) if i < self.core.config.rss.views.len() => {
                self.core.config.rss.views.remove(i);
                let _ = self.core.config.save();
                self.rebuild_sidebar_items();
            }
            _ => {}
        }
    }

    fn auto_scroll_view(&mut self, visible_height: u16) {
        let content_width = self.core.content_area.width.saturating_sub(6) as usize;
        let mut line_idx: usize = 0;
        for (i, article) in self.view_articles.iter().enumerate() {
            if i == self.view_selected {
                break;
            }
            if self.view_expanded == Some(i) {
                line_idx += crate::ui::expanded_article_lines(article, content_width);
            } else {
                line_idx += 1;
            }
        }
        let h = visible_height as usize;
        if line_idx < self.view_scroll as usize {
            self.view_scroll = line_idx as u16;
        } else if h > 0 && line_idx >= self.view_scroll as usize + h {
            self.view_scroll = (line_idx.saturating_sub(h) + 1) as u16;
        }
    }

    fn handle_create_view_key(&mut self, key: crossterm::event::KeyEvent) {
        match self.create_view_step {
            0 => match key.code {
                KeyCode::Esc => {
                    self.creating_view = false;
                }
                KeyCode::Enter if !self.create_view_name.trim().is_empty() => {
                    self.create_view_step = 1;
                    self.create_view_feeds = vec![false; self.feed_store.feeds.len()];
                    self.create_view_cursor = 0;
                }
                KeyCode::Backspace => {
                    self.create_view_name.pop();
                }
                KeyCode::Char(c) => {
                    self.create_view_name.push(c);
                }
                _ => {}
            },
            1 => match key.code {
                KeyCode::Esc => {
                    self.create_view_step = 0;
                }
                KeyCode::Enter => {
                    let name = self.create_view_name.trim().to_string();
                    let feeds: Vec<String> = self
                        .feed_store
                        .feeds
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.create_view_feeds.get(*i).copied().unwrap_or(false))
                        .map(|(_, f)| f.url.clone())
                        .collect();
                    if !name.is_empty() && !feeds.is_empty() {
                        self.core.config.rss.views.push(ViewConfig { name, feeds });
                        let _ = self.core.config.save();
                        self.rebuild_sidebar_items();
                    }
                    self.creating_view = false;
                    if self.mode == AppMode::Settings {
                        self.settings_items = settings::rebuild_items(self);
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(val) = self.create_view_feeds.get_mut(self.create_view_cursor) {
                        *val = !*val;
                    }
                }
                _ if event::is_nav_up(&key) => {
                    self.create_view_cursor = self.create_view_cursor.saturating_sub(1);
                }
                _ if event::is_nav_down(&key) && !self.create_view_feeds.is_empty() => {
                    self.create_view_cursor =
                        (self.create_view_cursor + 1).min(self.create_view_feeds.len() - 1);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn open_current_article_link(&self) {
        if let Some(feed) = self.feed_store.feeds.get(self.feed_index)
            && let Some(article) = feed.articles.get(self.article_index)
            && !article.link.is_empty()
        {
            let _ = open::that(&article.link);
        }
    }

    fn toggle_article_read(&mut self) {
        if let Some(feed) = self.feed_store.feeds.get_mut(self.feed_index)
            && let Some(article) = feed.articles.get_mut(self.article_index)
        {
            article.read = !article.read;
            let _ = self.db.set_read(article.id, article.read);
            if article.read {
                feed.unread_count = feed.unread_count.saturating_sub(1);
            } else {
                feed.unread_count += 1;
            }
        }
    }

    fn toggle_article_star(&mut self) {
        if let Some(feed) = self.feed_store.feeds.get_mut(self.feed_index)
            && let Some(article) = feed.articles.get_mut(self.article_index)
        {
            article.starred = !article.starred;
            let _ = self.db.set_starred(article.id, article.starred);
        }
    }

    fn refresh_feeds_background(&mut self) {
        if self.refreshing {
            return;
        } // already in progress
        if let Some(tx) = self.event_tx.clone() {
            self.refreshing = true;
            let feed_urls: Vec<(i64, String)> = self
                .feed_store
                .feeds
                .iter()
                .map(|f| (f.id, f.url.clone()))
                .collect();
            let config = Config::load().ok();
            tokio::spawn(async move {
                let results = fetch_all_feeds(&feed_urls).await;
                if config.is_some()
                    && let Ok(db) = Database::open_default()
                {
                    store_fetched_feeds(&db, &results);
                }
                // Always signal completion to clear refreshing state
                let _ = tx.send(AppEvent::Custom(AzideCustom::FeedsRefreshed));
            });
        }
    }

    fn mark_all_read_in_feed(&mut self) {
        if let Some(feed) = self.feed_store.feeds.get_mut(self.feed_index) {
            for article in &mut feed.articles {
                if !article.read {
                    article.read = true;
                    let _ = self.db.mark_read(article.id);
                }
            }
            feed.unread_count = 0;
        }
    }

    async fn handle_settings_action(&mut self, action: settings::SettingsAction) {
        match action {
            settings::SettingsAction::None => {}
            settings::SettingsAction::ExitSettings => {
                self.mode = AppMode::Reader;
                self.rebuild_sidebar_items();
            }
            settings::SettingsAction::AddFeed(url) => {
                let _ = self.feed_store.add_feed_url(&url, &self.db);
                self.feed_store.refresh_all(&self.db).await;
                self.settings_items = settings::rebuild_items(self);
            }
            settings::SettingsAction::DeleteFeed(id) => {
                let _ = self.db.delete_feed(id);
                self.feed_store =
                    FeedStore::new(&self.db).unwrap_or(FeedStore { feeds: Vec::new() });
                self.settings_items = settings::rebuild_items(self);
                // Find nearest selectable item
                if self.settings_index >= self.settings_items.len()
                    || !self.settings_items[self.settings_index].is_selectable()
                {
                    self.settings_index = settings::first_selectable(&self.settings_items);
                }
            }
            settings::SettingsAction::DeleteView(index) => {
                if index < self.core.config.rss.views.len() {
                    self.core.config.rss.views.remove(index);
                    let _ = self.core.config.save();
                    self.settings_items = settings::rebuild_items(self);
                    if self.settings_index >= self.settings_items.len()
                        || !self.settings_items[self.settings_index].is_selectable()
                    {
                        self.settings_index = settings::first_selectable(&self.settings_items);
                    }
                }
            }
            settings::SettingsAction::SaveFeedUrl(feed_id, new_url) => {
                // Update URL in-place, preserving article history
                let _ = self.db.update_feed_url(feed_id, &new_url);
                if let Some(feed) = self.feed_store.feeds.iter_mut().find(|f| f.id == feed_id) {
                    feed.url = new_url;
                }
                self.settings_items = settings::rebuild_items(self);
            }
            settings::SettingsAction::OpenCreateViewModal => {
                self.creating_view = true;
                self.create_view_step = 0;
                self.create_view_name.clear();
                self.create_view_feeds = vec![false; self.feed_store.feeds.len()];
                self.create_view_cursor = 0;
            }
            settings::SettingsAction::BackupDatabase => {
                if let Ok(path) = settings::backup_database() {
                    // Replace the BackupDatabase item with a success message temporarily
                    // For now, just log it (visible in the backup file existing)
                    let _ = path; // backup created successfully
                }
            }
            settings::SettingsAction::CycleTheme => {
                let next = theme::next_id(&self.core.config.ui.theme).to_string();
                self.core.config.ui.theme = next.clone();
                theme::set_current(&next);
                let _ = self.core.config.save();
                self.settings_items = settings::rebuild_items(self);
            }
        }
        // Auto-scroll after any action
        let visible_height = if self.core.content_area.height > 2 {
            self.core.content_area.height - 2
        } else {
            10
        };
        self.settings_scroll =
            settings::auto_scroll(self.settings_index, self.settings_scroll, visible_height);
    }

    async fn handle_explore_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char('?') => {
                self.core.help_visible = true;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Reader;
            }
            KeyCode::Tab => {
                self.explore_focus = match self.explore_focus {
                    ExploreFocus::CategoryList => ExploreFocus::FeedList,
                    ExploreFocus::FeedList => ExploreFocus::CategoryList,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => match self.explore_focus {
                ExploreFocus::CategoryList => {
                    self.explore_scroll(1, u16::MAX, u16::MAX);
                }
                ExploreFocus::FeedList => {
                    self.explore_scroll(1, u16::MAX, u16::MAX);
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.explore_focus {
                ExploreFocus::CategoryList => {
                    self.explore_scroll(-1, u16::MAX, u16::MAX);
                }
                ExploreFocus::FeedList => {
                    self.explore_scroll(-1, u16::MAX, u16::MAX);
                }
            },
            KeyCode::Enter if self.explore_focus == ExploreFocus::FeedList => {
                if let Some(cat) = self.explore_categories.get(self.explore_category_index)
                    && let Some(feed) = cat.feeds.get(self.explore_feed_index)
                {
                    let url = feed.url.clone();
                    let _ = self.feed_store.add_feed_url(&url, &self.db);
                    self.feed_store.refresh_all(&self.db).await;
                    self.rebuild_sidebar_items();
                    self.mode = AppMode::Reader;
                    // Select the newly added feed
                    if let Some(idx) = self.feed_store.feeds.iter().position(|f| f.url == url) {
                        self.feed_index = idx;
                        self.rss_view = RssView::ArticleList;
                        self.article_index = 0;
                        self.article_list_state.select(Some(0));
                        if let Some(sidebar_idx) = self
                            .sidebar_items
                            .iter()
                            .position(|item| matches!(item, SidebarItem::Feed(i) if *i == idx))
                        {
                            self.sidebar_index = sidebar_idx;
                            self.feed_list_state.select(Some(sidebar_idx));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
