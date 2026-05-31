use azide_config::{Config, ViewConfig};
use azide_feed::{ParsedFeed, fetch_all_feeds, generate_feed_tag};
use azide_store::{ArticleUpsert, Database, FeedArticleBatch, FeedRecord, StoredArticle};
use color_eyre::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const UNREAD_ARTICLE_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedSummary {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub unread_count: usize,
    pub article_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleRow {
    pub feed_id: i64,
    pub feed_title: String,
    pub feed_tag: String,
    pub article: Arc<StoredArticle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    All,
    Unread,
    Saved,
    Feed(i64),
    View(usize),
}

#[derive(Debug)]
pub struct GuiModel {
    pub config: Config,
    pub feeds: Vec<FeedSummary>,
    pub articles: Vec<ArticleRow>,
    pub selection: Selection,
    pub selected_article_id: Option<i64>,
    pub status: String,
    reader_article: Option<ArticleRow>,
    feed_index: HashMap<i64, usize>,
    article_index: HashMap<i64, usize>,
}

impl GuiModel {
    pub fn load(db: &Database) -> Result<Self> {
        let mut model = Self::from_config(Config::load()?);
        model.ensure_config_feeds(db)?;
        model.reload(db)?;
        Ok(model)
    }

    fn from_config(config: Config) -> Self {
        Self {
            config,
            feeds: Vec::new(),
            articles: Vec::new(),
            selection: Selection::All,
            selected_article_id: None,
            status: String::new(),
            reader_article: None,
            feed_index: HashMap::new(),
            article_index: HashMap::new(),
        }
    }

    pub fn reload(&mut self, db: &Database) -> Result<()> {
        let feeds = load_feed_data(db)?;
        self.feeds = feeds.iter().map(|feed| feed.summary.clone()).collect();
        self.feed_index = index_feeds(&self.feeds);

        let selection = normalize_selection(self.selection, &self.config, &self.feed_index);
        if selection != self.selection {
            self.selection = selection;
            self.selected_article_id = None;
        }

        self.articles = load_articles(db, &feeds, &self.config, self.selection)?;
        self.article_index = index_articles(&self.articles);
        self.reconcile_selected_article();
        Ok(())
    }

    pub fn select(&mut self, db: &Database, selection: Selection) -> Result<()> {
        self.selection = selection;
        self.selected_article_id = None;
        self.reader_article = None;
        self.reload(db)
    }

    pub fn selected_article(&self) -> Option<&ArticleRow> {
        let id = self.selected_article_id?;
        self.article_index
            .get(&id)
            .and_then(|index| self.articles.get(*index))
            .filter(|row| row.article.id == id)
            .or_else(|| self.articles.iter().find(|row| row.article.id == id))
            .or_else(|| {
                self.reader_article
                    .as_ref()
                    .filter(|row| row.article.id == id)
            })
    }

    pub fn select_article(&mut self, db: &Database, article_id: i64) -> Result<()> {
        self.selected_article_id = Some(article_id);
        self.reader_article = self
            .article_index
            .get(&article_id)
            .and_then(|index| self.articles.get(*index))
            .filter(|row| row.article.id == article_id)
            .cloned();
        self.set_article_read(db, article_id, true)
    }

    pub fn set_selected_starred(&mut self, db: &Database, starred: bool) -> Result<()> {
        if let Some(id) = self.selected_article_id {
            db.set_starred(id, starred)?;
            self.update_article_starred(id, starred);
        }
        Ok(())
    }

    pub fn set_selected_read(&mut self, db: &Database, read: bool) -> Result<()> {
        if let Some(id) = self.selected_article_id {
            self.set_article_read(db, id, read)?;
        }
        Ok(())
    }

    pub fn add_feed(&mut self, db: &Database, url: &str) -> Result<Option<i64>> {
        let url = url.trim();
        if url.is_empty() {
            return Ok(None);
        }
        let id = db.add_feed(url, url)?;
        if !self.config.rss.feeds.iter().any(|existing| existing == url) {
            self.config.rss.feeds.push(url.to_string());
            self.config.save()?;
        }
        self.status = format!("Added feed {url}");
        self.reload(db)?;
        Ok(Some(id))
    }

    pub fn delete_feed(&mut self, db: &Database, feed_id: i64) -> Result<()> {
        if let Some(url) = self.feed_by_id(feed_id).map(|feed| feed.url.clone()) {
            let selected_view_name = if let Selection::View(index) = self.selection {
                self.config
                    .rss
                    .views
                    .get(index)
                    .map(|view| view.name.clone())
            } else {
                None
            };
            self.config.rss.feeds.retain(|feed_url| feed_url != &url);
            for view in &mut self.config.rss.views {
                view.feeds.retain(|feed_url| feed_url != &url);
            }
            self.config.rss.views.retain(|view| !view.feeds.is_empty());
            if let Some(view_name) = selected_view_name {
                if let Some(index) = self
                    .config
                    .rss
                    .views
                    .iter()
                    .position(|view| view.name == view_name)
                {
                    self.selection = Selection::View(index);
                } else {
                    self.selection = Selection::All;
                    self.selected_article_id = None;
                    self.reader_article = None;
                }
            }
            self.config.save()?;
        }
        db.delete_feed(feed_id)?;
        if self
            .selected_article()
            .is_some_and(|row| row.feed_id == feed_id)
        {
            self.selected_article_id = None;
            self.reader_article = None;
        }
        if matches!(self.selection, Selection::Feed(id) if id == feed_id) {
            self.selection = Selection::All;
            self.selected_article_id = None;
            self.reader_article = None;
        }
        self.status = format!("Deleted feed id={feed_id}");
        self.reload(db)
    }

    pub fn add_explore_feed(&mut self, db: &Database, title: &str, url: &str) -> Result<()> {
        let id = db.add_feed(url, title)?;
        db.update_feed_title(id, title)?;
        if !self.config.rss.feeds.iter().any(|existing| existing == url) {
            self.config.rss.feeds.push(url.to_string());
            self.config.save()?;
        }
        self.status = format!("Added {title}");
        self.reload(db)
    }

    pub fn save_view(&mut self, db: &Database, name: &str, feed_ids: &[i64]) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || feed_ids.is_empty() {
            return Ok(());
        }
        let wanted_feed_ids = feed_ids.iter().copied().collect::<HashSet<_>>();
        let urls = self
            .feeds
            .iter()
            .filter(|feed| wanted_feed_ids.contains(&feed.id))
            .map(|feed| feed.url.clone())
            .collect::<Vec<_>>();
        if urls.is_empty() {
            return Ok(());
        }
        if let Some(view) = self
            .config
            .rss
            .views
            .iter_mut()
            .find(|view| view.name == name)
        {
            view.feeds = urls;
        } else {
            self.config.rss.views.push(ViewConfig {
                name: name.to_string(),
                feeds: urls,
            });
        }
        self.config.save()?;
        self.status = format!("Saved view {name}");
        self.reload(db)
    }

    pub fn delete_view(&mut self, db: &Database, index: usize) -> Result<()> {
        if index < self.config.rss.views.len() {
            let view = self.config.rss.views.remove(index);
            self.config.save()?;
            match self.selection {
                Selection::View(selected) if selected == index => {
                    self.selection = Selection::All;
                    self.selected_article_id = None;
                    self.reader_article = None;
                }
                Selection::View(selected) if selected > index => {
                    self.selection = Selection::View(selected - 1);
                    self.selected_article_id = None;
                    self.reader_article = None;
                }
                _ => {}
            }
            self.status = format!("Deleted view {}", view.name);
            self.reload(db)?;
        }
        Ok(())
    }

    pub async fn refresh_all(&mut self, db: &Database) -> Result<usize> {
        let feed_urls = self
            .feeds
            .iter()
            .map(|feed| (feed.id, feed.url.clone()))
            .collect::<Vec<_>>();
        if feed_urls.is_empty() {
            self.status = "No feeds to refresh".to_string();
            return Ok(0);
        }
        let fetched = fetch_all_feeds(&feed_urls).await;
        let count = fetched.iter().map(|feed| feed.articles.len()).sum();
        store_fetched_feeds(db, &fetched)?;
        self.reload(db)?;
        self.status = format!(
            "Refreshed {} feed(s), fetched {count} article(s)",
            fetched.len()
        );
        Ok(count)
    }

    fn ensure_config_feeds(&self, db: &Database) -> Result<()> {
        for url in &self.config.rss.feeds {
            db.add_feed(url, url)?;
        }
        Ok(())
    }

    fn feed_by_id(&self, feed_id: i64) -> Option<&FeedSummary> {
        self.feed_index
            .get(&feed_id)
            .and_then(|index| self.feeds.get(*index))
            .filter(|feed| feed.id == feed_id)
            .or_else(|| self.feeds.iter().find(|feed| feed.id == feed_id))
    }

    fn reconcile_selected_article(&mut self) {
        if let Some(id) = self.selected_article_id {
            if let Some(row) = self
                .article_index
                .get(&id)
                .and_then(|index| self.articles.get(*index))
                .filter(|row| row.article.id == id)
                .cloned()
            {
                self.reader_article = Some(row);
                return;
            }
            if self
                .reader_article
                .as_ref()
                .is_some_and(|row| row.article.id == id)
            {
                return;
            }
        }

        self.reader_article = self.articles.first().cloned();
        self.selected_article_id = self.reader_article.as_ref().map(|row| row.article.id);
    }

    fn set_article_read(&mut self, db: &Database, article_id: i64, read: bool) -> Result<()> {
        let Some(index) = self.article_index.get(&article_id).copied() else {
            db.set_read(article_id, read)?;
            self.reload(db)?;
            return Ok(());
        };
        let Some(row) = self.articles.get(index) else {
            db.set_read(article_id, read)?;
            self.reload(db)?;
            return Ok(());
        };
        let previous_read = row.article.read;
        let feed_id = row.feed_id;
        let mut reader_row = row.clone();
        if previous_read == read {
            return Ok(());
        }

        db.set_read(article_id, read)?;
        if self.selection == Selection::Unread && read {
            set_row_read(&mut reader_row, read);
            self.reader_article = Some(reader_row);
            self.reload(db)?;
            return Ok(());
        }

        if let Some(feed_index) = self.feed_index.get(&feed_id).copied()
            && let Some(feed) = self.feeds.get_mut(feed_index)
        {
            match (previous_read, read) {
                (false, true) => feed.unread_count = feed.unread_count.saturating_sub(1),
                (true, false) => feed.unread_count += 1,
                _ => {}
            }
        }
        self.update_article_read(article_id, read);
        Ok(())
    }

    fn update_article_read(&mut self, article_id: i64, read: bool) {
        self.update_article(article_id, |article| article.read = read);
        self.update_reader_article(article_id, |article| article.read = read);
        if self.selection == Selection::Unread && read {
            self.remove_article(article_id);
        }
    }

    fn update_article_starred(&mut self, article_id: i64, starred: bool) {
        self.update_article(article_id, |article| article.starred = starred);
        self.update_reader_article(article_id, |article| article.starred = starred);
        if self.selection == Selection::Saved && !starred {
            self.remove_article(article_id);
        }
    }

    fn update_article(&mut self, article_id: i64, update: impl FnOnce(&mut StoredArticle)) {
        if let Some(index) = self.article_index.get(&article_id).copied()
            && let Some(row) = self.articles.get_mut(index)
        {
            let mut article = (*row.article).clone();
            update(&mut article);
            row.article = Arc::new(article);
        }
    }

    fn update_reader_article(&mut self, article_id: i64, update: impl FnOnce(&mut StoredArticle)) {
        if let Some(row) = &mut self.reader_article
            && row.article.id == article_id
        {
            let mut article = (*row.article).clone();
            update(&mut article);
            row.article = Arc::new(article);
        }
    }

    fn remove_article(&mut self, article_id: i64) {
        let Some(index) = self.article_index.get(&article_id).copied() else {
            return;
        };
        if self
            .articles
            .get(index)
            .is_some_and(|row| row.article.id == article_id)
        {
            self.articles.remove(index);
            self.article_index = index_articles(&self.articles);
            self.reconcile_selected_article();
        }
    }
}

pub fn store_fetched_feeds(db: &Database, feeds: &[ParsedFeed]) -> Result<usize> {
    let articles = feeds
        .iter()
        .map(|feed| {
            feed.articles
                .iter()
                .map(|article| ArticleUpsert {
                    guid: &article.guid,
                    title: &article.title,
                    link: &article.link,
                    content: &article.content,
                    published: article.published,
                    author: &article.author,
                    categories: &article.categories,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let batches = feeds
        .iter()
        .zip(&articles)
        .map(|(feed, articles)| FeedArticleBatch {
            feed_id: feed.feed_id,
            title: feed.title.as_deref(),
            articles,
        })
        .collect::<Vec<_>>();
    let stats = db.upsert_feed_article_batches(&batches)?;
    Ok(stats.articles_upserted)
}

#[derive(Debug)]
struct FeedData {
    summary: FeedSummary,
    display_title: String,
    feed_tag: String,
}

fn load_feed_data(db: &Database) -> Result<Vec<FeedData>> {
    Ok(db
        .get_feed_records()?
        .into_iter()
        .map(feed_data_from_record)
        .collect())
}

fn feed_data_from_record(record: FeedRecord) -> FeedData {
    let display_title = display_feed_title(&record.url, &record.title);
    let feed_tag = generate_feed_tag(&display_title);
    FeedData {
        summary: FeedSummary {
            id: record.id,
            url: record.url,
            title: record.title,
            unread_count: record.unread_count,
            article_count: record.article_count,
        },
        display_title,
        feed_tag,
    }
}

fn index_feeds(feeds: &[FeedSummary]) -> HashMap<i64, usize> {
    feeds
        .iter()
        .enumerate()
        .map(|(index, feed)| (feed.id, index))
        .collect()
}

fn index_articles(articles: &[ArticleRow]) -> HashMap<i64, usize> {
    articles
        .iter()
        .enumerate()
        .map(|(index, row)| (row.article.id, index))
        .collect()
}

fn normalize_selection(
    selection: Selection,
    config: &Config,
    feed_index: &HashMap<i64, usize>,
) -> Selection {
    match selection {
        Selection::Feed(feed_id) if !feed_index.contains_key(&feed_id) => Selection::All,
        Selection::View(index) if index >= config.rss.views.len() => Selection::All,
        selection => selection,
    }
}

fn load_articles(
    db: &Database,
    feeds: &[FeedData],
    config: &Config,
    selection: Selection,
) -> Result<Vec<ArticleRow>> {
    let feed_by_id = feeds
        .iter()
        .map(|feed| (feed.summary.id, feed))
        .collect::<HashMap<_, _>>();
    let articles = match selection {
        Selection::All => db.get_all_articles_with_feeds()?,
        Selection::Unread => db.get_unread_articles(UNREAD_ARTICLE_LIMIT)?,
        Selection::Saved => db.get_starred_articles_with_feeds()?,
        Selection::Feed(feed_id) => db
            .get_articles(feed_id)?
            .into_iter()
            .map(|article| (feed_id, article))
            .collect(),
        Selection::View(index) => {
            let Some(view) = config.rss.views.get(index) else {
                return Ok(Vec::new());
            };
            let wanted = view.feeds.iter().collect::<HashSet<_>>();
            let feed_ids = feeds
                .iter()
                .filter(|feed| wanted.contains(&feed.summary.url))
                .map(|feed| feed.summary.id)
                .collect::<Vec<_>>();
            db.get_articles_for_feed_ids(&feed_ids)?
        }
    };

    Ok(articles
        .into_iter()
        .filter_map(|(feed_id, article)| {
            feed_by_id
                .get(&feed_id)
                .map(|feed| row_for_article(feed, article))
        })
        .collect())
}

fn row_for_article(feed: &FeedData, article: StoredArticle) -> ArticleRow {
    ArticleRow {
        feed_id: feed.summary.id,
        feed_title: feed.display_title.clone(),
        feed_tag: feed.feed_tag.clone(),
        article: Arc::new(article),
    }
}

fn set_row_read(row: &mut ArticleRow, read: bool) {
    let mut article = (*row.article).clone();
    article.read = read;
    row.article = Arc::new(article);
}

fn display_feed_title(url: &str, title: &str) -> String {
    if title.trim().is_empty() {
        url.to_string()
    } else {
        title.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use azide_feed::{ArticleContentKind, ParsedArticle};
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::open_at(dir.path().join("azide.db")).unwrap();
        (dir, db)
    }

    fn article_id(db: &Database, feed_id: i64, guid: &str) -> i64 {
        db.get_articles(feed_id)
            .unwrap()
            .into_iter()
            .find(|article| article.guid == guid)
            .unwrap()
            .id
    }

    #[test]
    fn store_fetched_feeds_updates_title_and_articles() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "old").unwrap();

        let stored = store_fetched_feeds(
            &db,
            &[ParsedFeed {
                feed_id,
                title: Some("Example".into()),
                description: None,
                articles: vec![ParsedArticle {
                    guid: "a".into(),
                    title: "Article".into(),
                    link: "https://example.test/a".into(),
                    content: "<p>Hello</p>".into(),
                    content_kind: ArticleContentKind::Content,
                    published: 42,
                    author: "Author".into(),
                    categories: "rust".into(),
                }],
            }],
        )
        .unwrap();

        assert_eq!(stored, 1);
        assert_eq!(db.get_feeds().unwrap()[0].2, "Example");
        let article = db.get_articles(feed_id).unwrap().remove(0);
        assert_eq!(article.title, "Article");
        assert_eq!(article.author, "Author");
    }

    #[test]
    fn model_loads_rows_and_toggles_state() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "a", "Article", "", "body", 10, "", "")
            .unwrap();

        let mut model = GuiModel::from_config(Config::default());
        model.reload(&db).unwrap();

        assert_eq!(model.feeds[0].unread_count, 1);
        assert_eq!(model.feeds[0].article_count, 1);
        let article_id = model.articles[0].article.id;
        let cloned_row = model.articles[0].clone();
        assert!(Arc::ptr_eq(&cloned_row.article, &model.articles[0].article));
        model.select_article(&db, article_id).unwrap();
        assert!(model.selected_article().unwrap().article.read);
        model.set_selected_starred(&db, true).unwrap();
        assert!(model.selected_article().unwrap().article.starred);
    }

    #[test]
    fn reload_normalizes_stale_feed_and_view_selection() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "a", "Article", "", "body", 10, "", "")
            .unwrap();
        let expected_article_id = article_id(&db, feed_id, "a");

        let mut model = GuiModel::from_config(Config::default());
        model.selection = Selection::Feed(feed_id + 100);
        model.selected_article_id = Some(-1);
        model.reload(&db).unwrap();

        assert_eq!(model.selection, Selection::All);
        assert_eq!(model.selected_article_id, Some(expected_article_id));

        model.selection = Selection::View(10);
        model.selected_article_id = Some(-1);
        model.reload(&db).unwrap();

        assert_eq!(model.selection, Selection::All);
        assert_eq!(model.selected_article_id, Some(expected_article_id));
    }

    #[test]
    fn view_selection_uses_loaded_config_without_reloading_from_disk() {
        let (_dir, db) = temp_db();
        let first = db.add_feed("https://first.example/rss", "First").unwrap();
        let second = db.add_feed("https://second.example/rss", "Second").unwrap();
        db.upsert_article(first, "first", "First article", "", "", 10, "", "")
            .unwrap();
        db.upsert_article(second, "second", "Second article", "", "", 20, "", "")
            .unwrap();

        let mut config = Config::default();
        config.rss.views.push(ViewConfig {
            name: "Second only".into(),
            feeds: vec!["https://second.example/rss".into()],
        });
        let mut model = GuiModel::from_config(config);
        model.selection = Selection::View(0);
        model.reload(&db).unwrap();

        assert_eq!(model.selection, Selection::View(0));
        assert_eq!(model.articles.len(), 1);
        assert_eq!(model.articles[0].feed_id, second);
        assert_eq!(model.articles[0].article.guid, "second");
    }

    #[test]
    fn unread_selection_replaces_selected_article_after_marking_read() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "old", "Old", "", "", 10, "", "")
            .unwrap();
        db.upsert_article(feed_id, "new", "New", "", "", 20, "", "")
            .unwrap();

        let mut model = GuiModel::from_config(Config::default());
        model.select(&db, Selection::Unread).unwrap();
        let selected = model.selected_article_id.unwrap();
        assert_eq!(selected, article_id(&db, feed_id, "new"));

        model.select_article(&db, selected).unwrap();

        assert_eq!(model.selection, Selection::Unread);
        assert_eq!(model.articles.len(), 1);
        assert!(model.articles.iter().all(|row| !row.article.read));
        assert_eq!(model.selected_article_id, Some(selected));
        let reader = model.selected_article().unwrap();
        assert_eq!(reader.article.id, selected);
        assert!(reader.article.read);
        assert!(!model.articles.iter().any(|row| row.article.id == selected));
        assert_eq!(
            model.articles[0].article.id,
            article_id(&db, feed_id, "old")
        );
    }

    #[test]
    fn unread_selection_backfills_bounded_window_after_marking_read() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        for index in 0..501 {
            db.upsert_article(
                feed_id,
                &format!("article-{index}"),
                &format!("Article {index}"),
                "",
                "",
                index,
                "",
                "",
            )
            .unwrap();
        }

        let mut model = GuiModel::from_config(Config::default());
        model.select(&db, Selection::Unread).unwrap();
        assert_eq!(model.articles.len(), 500);
        let selected = model.selected_article_id.unwrap();
        assert_eq!(selected, article_id(&db, feed_id, "article-500"));

        model.select_article(&db, selected).unwrap();

        assert_eq!(model.selection, Selection::Unread);
        assert_eq!(model.articles.len(), 500);
        assert!(model.articles.iter().all(|row| !row.article.read));
        assert!(!model.articles.iter().any(|row| row.article.id == selected));
        assert_eq!(model.selected_article_id, Some(selected));
        let reader = model.selected_article().unwrap();
        assert_eq!(reader.article.id, selected);
        assert!(reader.article.read);
        assert_eq!(
            model.articles[0].article.id,
            article_id(&db, feed_id, "article-499")
        );
    }
}
