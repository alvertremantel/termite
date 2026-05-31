use azide_feed::{ParsedFeed, fetch_all_feeds};
use azide_store::{Database, StoredArticle};

pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub articles: Vec<StoredArticle>,
    pub unread_count: usize,
}

pub struct FeedStore {
    pub feeds: Vec<Feed>,
}

impl FeedStore {
    pub fn new(db: &Database) -> color_eyre::Result<Self> {
        Ok(Self {
            feeds: load_feeds(db)?,
        })
    }

    pub fn article_count(&self, feed_index: usize) -> usize {
        self.feeds
            .get(feed_index)
            .map(|feed| feed.articles.len())
            .unwrap_or(0)
    }

    pub fn mark_read(&mut self, feed_index: usize, article_index: usize, db: &Database) {
        if let Some(feed) = self.feeds.get_mut(feed_index)
            && let Some(article) = feed.articles.get_mut(article_index)
            && !article.read
        {
            article.read = true;
            feed.unread_count = feed.unread_count.saturating_sub(1);
            let _ = db.mark_read(article.id);
        }
    }

    pub async fn refresh_all(&mut self, db: &Database) {
        let feed_urls: Vec<(i64, String)> = self
            .feeds
            .iter()
            .map(|feed| (feed.id, feed.url.clone()))
            .collect();
        let results = fetch_all_feeds(&feed_urls).await;
        self.apply_fetched(db, &results);
    }

    /// Inject pre-fetched or fixture `ParsedFeed` slices directly, bypassing live HTTP.
    /// Articles are stored and the in-memory view is reloaded from the database.
    pub fn apply_fetched(&mut self, db: &Database, results: &[ParsedFeed]) {
        store_fetched_feeds(db, results);
        self.reload_from_db(db);
    }

    pub fn reload_from_db(&mut self, db: &Database) {
        if let Ok(feeds) = load_feeds(db) {
            self.feeds = feeds;
        }
    }

    pub fn add_feed_url(&mut self, url: &str, db: &Database) -> color_eyre::Result<()> {
        let id = db.add_feed(url, url)?;
        if !self.feeds.iter().any(|feed| feed.id == id) {
            self.feeds.push(Feed {
                id,
                url: url.to_string(),
                title: url.to_string(),
                articles: Vec::new(),
                unread_count: 0,
            });
        }
        Ok(())
    }
}

pub fn store_parsed_feed(db: &Database, feed: &ParsedFeed) {
    if let Some(title) = &feed.title {
        let _ = db.update_feed_title(feed.feed_id, title);
    }
    for article in &feed.articles {
        let _ = db.upsert_article(
            feed.feed_id,
            &article.guid,
            &article.title,
            &article.link,
            &article.content,
            article.published,
            &article.author,
            &article.categories,
        );
    }
}

pub fn store_fetched_feeds(db: &Database, feeds: &[ParsedFeed]) {
    for feed in feeds {
        store_parsed_feed(db, feed);
    }
}

fn load_feeds(db: &Database) -> color_eyre::Result<Vec<Feed>> {
    let db_feeds = db.get_feeds()?;
    let mut feeds = Vec::new();
    for (id, url, title) in db_feeds {
        feeds.push(Feed {
            id,
            url,
            title,
            articles: db.get_articles(id).unwrap_or_default(),
            unread_count: db.unread_count(id),
        });
    }
    Ok(feeds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::open_at(dir.path().join("azide.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn new_and_article_count_reflect_database_state() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "guid-1", "One", "", "", 10, "", "")
            .unwrap();

        let store = FeedStore::new(&db).unwrap();

        assert_eq!(store.feeds.len(), 1);
        assert_eq!(store.article_count(0), 1);
        assert_eq!(store.feeds[0].unread_count, 1);
    }

    #[test]
    fn mark_read_updates_memory_and_database() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "guid-1", "One", "", "", 10, "", "")
            .unwrap();
        let mut store = FeedStore::new(&db).unwrap();

        store.mark_read(0, 0, &db);

        assert!(store.feeds[0].articles[0].read);
        assert_eq!(store.feeds[0].unread_count, 0);
        assert!(db.get_articles(feed_id).unwrap()[0].read);
    }

    #[test]
    fn add_feed_url_avoids_duplicate_in_memory_entries() {
        let (_dir, db) = temp_db();
        let mut store = FeedStore::new(&db).unwrap();

        store.add_feed_url("https://example.test/rss", &db).unwrap();
        store.add_feed_url("https://example.test/rss", &db).unwrap();

        assert_eq!(store.feeds.len(), 1);
    }

    #[test]
    fn reload_from_db_applies_stored_feed_results() {
        let (_dir, db) = temp_db();
        let feed_id = db
            .add_feed("https://example.test/rss", "https://example.test/rss")
            .unwrap();
        let mut store = FeedStore::new(&db).unwrap();

        store_fetched_feeds(
            &db,
            &[ParsedFeed {
                feed_id,
                title: Some("Parsed Feed".into()),
                description: Some("desc".into()),
                articles: vec![azide_feed::ParsedArticle {
                    guid: "guid-1".into(),
                    title: "Article".into(),
                    link: "https://example.test/1".into(),
                    content: "body".into(),
                    content_kind: azide_feed::ArticleContentKind::Content,
                    published: 99,
                    author: "Author".into(),
                    categories: "rust".into(),
                }],
            }],
        );
        store.reload_from_db(&db);

        assert_eq!(store.feeds[0].title, "Parsed Feed");
        assert_eq!(store.article_count(0), 1);
        assert_eq!(store.feeds[0].articles[0].author, "Author");
    }

    #[test]
    fn apply_fetched_persists_then_reloads_store_view() {
        let (_dir, db) = temp_db();
        let feed_id = db
            .add_feed("https://example.test/rss", "https://example.test/rss")
            .unwrap();
        let mut store = FeedStore::new(&db).unwrap();

        // Inject fixture data via the injectable helper — no live HTTP needed.
        store.apply_fetched(
            &db,
            &[ParsedFeed {
                feed_id,
                title: Some("Fixture Feed".into()),
                description: Some("desc".into()),
                articles: vec![
                    azide_feed::ParsedArticle {
                        guid: "guid-a".into(),
                        title: "Article A".into(),
                        link: "https://example.test/a".into(),
                        content: "content a".into(),
                        content_kind: azide_feed::ArticleContentKind::Content,
                        published: 200,
                        author: "Author A".into(),
                        categories: "rust, news".into(),
                    },
                    azide_feed::ParsedArticle {
                        guid: "guid-b".into(),
                        title: "Article B".into(),
                        link: "https://example.test/b".into(),
                        content: "content b".into(),
                        content_kind: azide_feed::ArticleContentKind::Summary,
                        published: 100,
                        author: "Author B".into(),
                        categories: "release".into(),
                    },
                ],
            }],
        );

        assert_eq!(store.feeds.len(), 1);
        assert_eq!(store.feeds[0].title, "Fixture Feed");
        assert_eq!(store.article_count(0), 2);
        assert_eq!(store.feeds[0].articles[0].title, "Article A");
        assert_eq!(store.feeds[0].articles[0].author, "Author A");
        assert_eq!(store.feeds[0].articles[0].categories, "rust, news");
        assert_eq!(store.feeds[0].articles[1].title, "Article B");
    }
}
