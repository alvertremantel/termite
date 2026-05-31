use color_eyre::{Result, eyre::bail};
use rusqlite::{Connection, params};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;

const MAX_FEED_ID_QUERY_PARAMS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRecord {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub unread_count: usize,
    pub article_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewArticle {
    pub id: i64,
    pub feed_id: i64,
    pub feed_tag: String,
    pub title: String,
    pub link: String,
    pub content: String,
    pub published: i64,
    pub read: bool,
    pub starred: bool,
    pub author: String,
    pub categories: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArticle {
    pub id: i64,
    pub guid: String,
    pub title: String,
    pub link: String,
    pub content: String,
    pub published: i64,
    pub read: bool,
    pub starred: bool,
    pub author: String,
    pub categories: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArticleUpsert<'a> {
    pub guid: &'a str,
    pub title: &'a str,
    pub link: &'a str,
    pub content: &'a str,
    pub published: i64,
    pub author: &'a str,
    pub categories: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedArticleBatch<'a> {
    pub feed_id: i64,
    pub title: Option<&'a str>,
    pub articles: &'a [ArticleUpsert<'a>],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpsertStats {
    pub feeds_updated: usize,
    pub articles_upserted: usize,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open_default() -> Result<Self> {
        let data_dir = azide_config::Config::data_dir();
        std::fs::create_dir_all(&data_dir)?;
        Self::open_at(data_dir.join("azide.db"))
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS feeds (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                etag TEXT,
                last_modified TEXT,
                last_fetched INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY,
                feed_id INTEGER NOT NULL REFERENCES feeds(id),
                guid TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                link TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                published INTEGER DEFAULT 0,
                read INTEGER DEFAULT 0,
                starred INTEGER DEFAULT 0,
                UNIQUE(feed_id, guid)
            );
            CREATE INDEX IF NOT EXISTS idx_articles_feed_published
                ON articles(feed_id, published DESC);
            CREATE INDEX IF NOT EXISTS idx_articles_published
                ON articles(published DESC);
            CREATE INDEX IF NOT EXISTS idx_articles_read_published
                ON articles(read, published DESC);
            CREATE INDEX IF NOT EXISTS idx_articles_starred_published
                ON articles(starred, published DESC);
            CREATE INDEX IF NOT EXISTS idx_articles_unread_published_partial
                ON articles(published DESC) WHERE read = 0;
            CREATE INDEX IF NOT EXISTS idx_articles_starred_published_partial
                ON articles(published DESC) WHERE starred = 1;",
        )?;
        // Migrate: add `author` column only if it does not already exist.
        let has_author: i64 = self
            .conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'author'")?
            .query_row([], |row| row.get(0))?;
        if has_author == 0 {
            self.conn.execute(
                "ALTER TABLE articles ADD COLUMN author TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        // Migrate: add `categories` column only if it does not already exist.
        let has_categories: i64 = self
            .conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = 'categories'",
            )?
            .query_row([], |row| row.get(0))?;
        if has_categories == 0 {
            self.conn.execute(
                "ALTER TABLE articles ADD COLUMN categories TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        Ok(())
    }

    pub fn add_feed(&self, url: &str, title: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO feeds (url, title) VALUES (?1, ?2)",
            params![url, title],
        )?;
        Ok(self
            .conn
            .query_row("SELECT id FROM feeds WHERE url = ?1", [url], |row| {
                row.get(0)
            })?)
    }

    pub fn get_feeds(&self) -> Result<Vec<(i64, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url, title FROM feeds ORDER BY title")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_feed_records(&self) -> Result<Vec<FeedRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.url, f.title,
                    COALESCE(SUM(CASE WHEN a.read = 0 THEN 1 ELSE 0 END), 0) AS unread_count,
                    COUNT(a.id) AS article_count
             FROM feeds f
             LEFT JOIN articles a ON a.feed_id = f.id
             GROUP BY f.id, f.url, f.title
             ORDER BY f.title",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FeedRecord {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                unread_count: row.get(3)?,
                article_count: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_feed_by_url(&self, url: &str) -> Result<Option<(i64, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url, title FROM feeds WHERE url = ?1")?;
        let mut rows = stmt.query_map([url], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn update_feed_title(&self, feed_id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET title = ?2 WHERE id = ?1",
            params![feed_id, title],
        )?;
        Ok(())
    }

    pub fn update_feed_url(&self, feed_id: i64, url: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET url = ?2 WHERE id = ?1",
            params![feed_id, url],
        )?;
        Ok(())
    }

    pub fn delete_feed(&self, feed_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM articles WHERE feed_id = ?1", [feed_id])?;
        self.conn
            .execute("DELETE FROM feeds WHERE id = ?1", [feed_id])?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_article(
        &self,
        feed_id: i64,
        guid: &str,
        title: &str,
        link: &str,
        content: &str,
        published: i64,
        author: &str,
        categories: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO articles (feed_id, guid, title, link, content, published, author, categories)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(feed_id, guid) DO UPDATE SET
                title = excluded.title, link = excluded.link, content = excluded.content,
                published = excluded.published, author = excluded.author, categories = excluded.categories",
            params![feed_id, guid, title, link, content, published, author, categories],
        )?;
        Ok(())
    }

    pub fn upsert_feed_article_batches(
        &self,
        batches: &[FeedArticleBatch<'_>],
    ) -> Result<UpsertStats> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stats = UpsertStats::default();
        {
            let mut update_feed = tx.prepare("UPDATE feeds SET title = ?2 WHERE id = ?1")?;
            let mut upsert_article = tx.prepare(
                "INSERT INTO articles (feed_id, guid, title, link, content, published, author, categories)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(feed_id, guid) DO UPDATE SET
                    title = excluded.title, link = excluded.link, content = excluded.content,
                    published = excluded.published, author = excluded.author, categories = excluded.categories",
            )?;

            for batch in batches {
                let feed_exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM feeds WHERE id = ?1)",
                    [batch.feed_id],
                    |row| row.get(0),
                )?;
                if !feed_exists {
                    bail!(
                        "cannot upsert articles for missing feed id {}",
                        batch.feed_id
                    );
                }
                if let Some(title) = batch.title {
                    stats.feeds_updated += update_feed.execute(params![batch.feed_id, title])?;
                }
                for article in batch.articles {
                    upsert_article.execute(params![
                        batch.feed_id,
                        article.guid,
                        article.title,
                        article.link,
                        article.content,
                        article.published,
                        article.author,
                        article.categories,
                    ])?;
                    stats.articles_upserted += 1;
                }
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    pub fn get_articles(&self, feed_id: i64) -> Result<Vec<StoredArticle>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE feed_id = ?1 ORDER BY published DESC",
        )?;
        let rows = stmt.query_map([feed_id], map_stored_article)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_all_articles_with_feeds(&self) -> Result<Vec<(i64, StoredArticle)>> {
        let mut stmt = self.conn.prepare(
            "SELECT feed_id, id, guid, title, link, content, published, read, starred, author, categories
             FROM articles ORDER BY published DESC",
        )?;
        let rows = stmt.query_map([], map_feed_article)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_article_by_id(&self, article_id: i64) -> Result<Option<StoredArticle>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([article_id], map_stored_article)?;
        match rows.next() {
            Some(Ok(row)) => Ok(Some(row)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn get_starred_articles(&self) -> Result<Vec<StoredArticle>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE starred = 1 ORDER BY published DESC",
        )?;
        let rows = stmt.query_map([], map_stored_article)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_starred_articles_with_feeds(&self) -> Result<Vec<(i64, StoredArticle)>> {
        let mut stmt = self.conn.prepare(
            "SELECT feed_id, id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE starred = 1 ORDER BY published DESC",
        )?;
        let rows = stmt.query_map([], map_feed_article)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_unread_articles(&self, limit: usize) -> Result<Vec<(i64, StoredArticle)>> {
        let mut stmt = self.conn.prepare(
            "SELECT feed_id, id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE read = 0 ORDER BY published DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                StoredArticle {
                    id: row.get(1)?,
                    guid: row.get(2)?,
                    title: row.get(3)?,
                    link: row.get(4)?,
                    content: row.get(5)?,
                    published: row.get(6)?,
                    read: row.get(7)?,
                    starred: row.get(8)?,
                    author: row.get(9)?,
                    categories: row.get(10)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_articles_for_feed_ids(&self, feed_ids: &[i64]) -> Result<Vec<(i64, StoredArticle)>> {
        if feed_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut articles = Vec::new();
        for chunk in feed_ids.chunks(MAX_FEED_ID_QUERY_PARAMS) {
            articles.extend(self.get_articles_for_feed_id_chunk(chunk)?);
        }
        articles.sort_by_key(|(_, article)| Reverse(article.published));
        Ok(articles)
    }

    fn get_articles_for_feed_id_chunk(
        &self,
        feed_ids: &[i64],
    ) -> Result<Vec<(i64, StoredArticle)>> {
        let placeholders = feed_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT feed_id, id, guid, title, link, content, published, read, starred, author, categories
             FROM articles WHERE feed_id IN ({placeholders}) ORDER BY published DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params = feed_ids
            .iter()
            .copied()
            .map(rusqlite::types::Value::Integer);
        let rows = stmt.query_map(rusqlite::params_from_iter(params), map_feed_article)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_articles_for_feeds(
        &self,
        feed_ids: &[(i64, String)],
        limit: usize,
    ) -> Result<Vec<ViewArticle>> {
        if feed_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = feed_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT a.id, a.feed_id, a.guid, a.title, a.link, a.content, a.published, a.read, a.starred, a.author, a.categories
             FROM articles a WHERE a.feed_id IN ({placeholders}) ORDER BY a.published DESC LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let tag_map: HashMap<i64, &str> = feed_ids
            .iter()
            .map(|(id, tag)| (*id, tag.as_str()))
            .collect();
        let mut param_values: Vec<rusqlite::types::Value> = feed_ids
            .iter()
            .map(|(id, _)| rusqlite::types::Value::Integer(*id))
            .collect();
        param_values.push(rusqlite::types::Value::Integer(limit as i64));

        let rows = stmt.query_map(rusqlite::params_from_iter(param_values), |row| {
            Ok(ViewArticle {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                feed_tag: String::new(),
                title: row.get(3)?,
                link: row.get(4)?,
                content: row.get(5)?,
                published: row.get(6)?,
                read: row.get(7)?,
                starred: row.get(8)?,
                author: row.get(9)?,
                categories: row.get(10)?,
            })
        })?;

        let mut articles = Vec::new();
        for result in rows {
            let mut article = result?;
            article.feed_tag = tag_map
                .get(&article.feed_id)
                .copied()
                .unwrap_or("???")
                .to_string();
            articles.push(article);
        }
        Ok(articles)
    }

    pub fn mark_read(&self, article_id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE articles SET read = 1 WHERE id = ?1", [article_id])?;
        Ok(())
    }

    pub fn set_read(&self, article_id: i64, read: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE articles SET read = ?2 WHERE id = ?1",
            params![article_id, read as i32],
        )?;
        Ok(())
    }

    pub fn set_starred(&self, article_id: i64, starred: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE articles SET starred = ?2 WHERE id = ?1",
            params![article_id, starred as i32],
        )?;
        Ok(())
    }

    pub fn unread_count(&self, feed_id: i64) -> usize {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM articles WHERE feed_id = ?1 AND read = 0",
                [feed_id],
                |row| row.get::<_, usize>(0),
            )
            .unwrap_or(0)
    }
}

fn map_stored_article(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredArticle> {
    Ok(StoredArticle {
        id: row.get(0)?,
        guid: row.get(1)?,
        title: row.get(2)?,
        link: row.get(3)?,
        content: row.get(4)?,
        published: row.get(5)?,
        read: row.get(6)?,
        starred: row.get(7)?,
        author: row.get(8)?,
        categories: row.get(9)?,
    })
}

fn map_feed_article(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, StoredArticle)> {
    Ok((
        row.get(0)?,
        StoredArticle {
            id: row.get(1)?,
            guid: row.get(2)?,
            title: row.get(3)?,
            link: row.get(4)?,
            content: row.get(5)?,
            published: row.get(6)?,
            read: row.get(7)?,
            starred: row.get(8)?,
            author: row.get(9)?,
            categories: row.get(10)?,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::open_at(dir.path().join("azide.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn open_at_creates_schema() {
        let (_dir, db) = temp_db();

        let feeds: usize = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('feeds', 'articles')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(feeds, 2);
    }

    #[test]
    fn add_and_get_feeds() {
        let (_dir, db) = temp_db();

        let first = db.add_feed("https://b.example/rss", "Bravo").unwrap();
        let second = db.add_feed("https://a.example/rss", "Alpha").unwrap();

        assert!(first > 0);
        assert!(second > 0);
        assert_eq!(
            db.get_feeds().unwrap(),
            vec![
                (second, "https://a.example/rss".into(), "Alpha".into()),
                (first, "https://b.example/rss".into(), "Bravo".into())
            ]
        );
        assert_eq!(
            db.get_feed_by_url("https://a.example/rss").unwrap(),
            Some((second, "https://a.example/rss".into(), "Alpha".into()))
        );
        assert_eq!(
            db.get_feed_records().unwrap(),
            vec![
                FeedRecord {
                    id: second,
                    url: "https://a.example/rss".into(),
                    title: "Alpha".into(),
                    unread_count: 0,
                    article_count: 0,
                },
                FeedRecord {
                    id: first,
                    url: "https://b.example/rss".into(),
                    title: "Bravo".into(),
                    unread_count: 0,
                    article_count: 0,
                }
            ]
        );
    }

    #[test]
    fn upsert_and_get_articles() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();

        db.upsert_article(
            feed_id,
            "guid-1",
            "First title",
            "https://example.test/1",
            "hello",
            10,
            "Author A",
            "news, rust",
        )
        .unwrap();
        db.upsert_article(
            feed_id,
            "guid-1",
            "Updated title",
            "https://example.test/1b",
            "updated",
            20,
            "Author B",
            "updates",
        )
        .unwrap();

        let articles = db.get_articles(feed_id).unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].title, "Updated title");
        assert_eq!(articles[0].author, "Author B");
        assert_eq!(articles[0].categories, "updates");
        assert_eq!(
            db.get_article_by_id(articles[0].id).unwrap(),
            Some(articles[0].clone())
        );
        assert_eq!(
            db.get_all_articles_with_feeds().unwrap(),
            vec![(feed_id, articles[0].clone())]
        );
    }

    #[test]
    fn batch_upsert_updates_feed_and_articles() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Old").unwrap();
        let articles = [ArticleUpsert {
            guid: "guid-1",
            title: "Title",
            link: "https://example.test/1",
            content: "body",
            published: 10,
            author: "Author",
            categories: "rust",
        }];

        let stats = db
            .upsert_feed_article_batches(&[FeedArticleBatch {
                feed_id,
                title: Some("New"),
                articles: &articles,
            }])
            .unwrap();

        assert_eq!(
            stats,
            UpsertStats {
                feeds_updated: 1,
                articles_upserted: 1,
            }
        );
        assert_eq!(db.get_feeds().unwrap()[0].2, "New");
        assert_eq!(db.get_articles(feed_id).unwrap()[0].author, "Author");
    }

    #[test]
    fn batch_upsert_rejects_missing_feed_without_orphans() {
        let (_dir, db) = temp_db();
        let articles = [ArticleUpsert {
            guid: "guid-1",
            title: "Title",
            link: "https://example.test/1",
            content: "body",
            published: 10,
            author: "Author",
            categories: "rust",
        }];

        let error = db
            .upsert_feed_article_batches(&[FeedArticleBatch {
                feed_id: 999,
                title: Some("Missing"),
                articles: &articles,
            }])
            .unwrap_err();

        assert!(error.to_string().contains("missing feed id 999"));
        assert!(db.get_all_articles_with_feeds().unwrap().is_empty());
    }

    #[test]
    fn foreign_keys_are_enabled_for_direct_article_upserts() {
        let (_dir, db) = temp_db();

        assert!(
            db.upsert_article(999, "guid-1", "Title", "", "", 10, "", "")
                .is_err()
        );
        assert!(db.get_all_articles_with_feeds().unwrap().is_empty());
    }

    #[test]
    fn read_and_star_toggles_work() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(
            feed_id,
            "guid-1",
            "Title",
            "https://example.test/1",
            "hello",
            10,
            "",
            "",
        )
        .unwrap();

        let article = db.get_articles(feed_id).unwrap().remove(0);
        assert_eq!(db.unread_count(feed_id), 1);

        db.mark_read(article.id).unwrap();
        assert!(db.get_article_by_id(article.id).unwrap().unwrap().read);
        db.set_read(article.id, false).unwrap();
        assert!(!db.get_article_by_id(article.id).unwrap().unwrap().read);
        db.set_starred(article.id, true).unwrap();
        assert!(db.get_starred_articles().unwrap()[0].starred);
        db.set_starred(article.id, false).unwrap();
        assert!(db.get_starred_articles().unwrap().is_empty());
    }

    #[test]
    fn delete_feed_cascades_manually() {
        let (_dir, db) = temp_db();
        let feed_id = db.add_feed("https://example.test/rss", "Example").unwrap();
        db.upsert_article(feed_id, "guid-1", "Title", "", "", 10, "", "")
            .unwrap();

        db.delete_feed(feed_id).unwrap();

        assert!(db.get_feeds().unwrap().is_empty());
        assert!(db.get_articles(feed_id).unwrap().is_empty());
    }

    #[test]
    fn unread_queries_and_view_queries_work() {
        let (_dir, db) = temp_db();
        let feed_a = db.add_feed("https://a.example/rss", "Alpha").unwrap();
        let feed_b = db.add_feed("https://b.example/rss", "Beta").unwrap();
        db.upsert_article(feed_a, "a1", "A1", "", "", 10, "", "rust")
            .unwrap();
        db.upsert_article(feed_a, "a2", "A2", "", "", 20, "", "")
            .unwrap();
        db.upsert_article(feed_b, "b1", "B1", "", "", 30, "", "ops")
            .unwrap();

        let a1 = db.get_articles(feed_a).unwrap().pop().unwrap();
        db.mark_read(a1.id).unwrap();

        let unread = db.get_unread_articles(10).unwrap();
        assert_eq!(unread.len(), 2);
        assert_eq!(db.unread_count(feed_a), 1);
        assert_eq!(db.unread_count(feed_b), 1);
        assert_eq!(db.get_feed_records().unwrap()[0].article_count, 2);
        assert_eq!(db.get_feed_records().unwrap()[0].unread_count, 1);

        let view_articles = db
            .get_articles_for_feeds(&[(feed_a, "ALP".into()), (feed_b, "BET".into())], 10)
            .unwrap();
        assert_eq!(view_articles.len(), 3);
        assert_eq!(view_articles[0].feed_id, feed_b);
        assert_eq!(view_articles[0].feed_tag, "BET");
        assert_eq!(view_articles[2].feed_tag, "ALP");

        let feed_articles = db.get_articles_for_feed_ids(&[feed_a]).unwrap();
        assert_eq!(feed_articles.len(), 2);
        assert!(feed_articles.iter().all(|(feed_id, _)| *feed_id == feed_a));
    }

    #[test]
    fn feed_id_article_query_handles_more_than_one_chunk() {
        let (_dir, db) = temp_db();
        let mut feed_ids = Vec::new();
        for index in 0..=MAX_FEED_ID_QUERY_PARAMS {
            let feed_id = db
                .add_feed(&format!("https://{index}.example/rss"), "")
                .unwrap();
            feed_ids.push(feed_id);
        }
        let first = feed_ids[0];
        let last = *feed_ids.last().unwrap();
        db.upsert_article(first, "first", "First", "", "", 10, "", "")
            .unwrap();
        db.upsert_article(last, "last", "Last", "", "", 20, "", "")
            .unwrap();

        let articles = db.get_articles_for_feed_ids(&feed_ids).unwrap();

        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].0, last);
        assert_eq!(articles[1].0, first);
    }

    #[test]
    fn legacy_author_and_categories_columns_are_migrated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("legacy.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE feeds (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                etag TEXT,
                last_modified TEXT,
                last_fetched INTEGER DEFAULT 0
            );
            CREATE TABLE articles (
                id INTEGER PRIMARY KEY,
                feed_id INTEGER NOT NULL REFERENCES feeds(id),
                guid TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                link TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                published INTEGER DEFAULT 0,
                read INTEGER DEFAULT 0,
                starred INTEGER DEFAULT 0,
                UNIQUE(feed_id, guid)
            );
            INSERT INTO feeds (id, url, title) VALUES (1, 'https://example.test/rss', 'Legacy');
            INSERT INTO articles (feed_id, guid, title, link, content, published, read, starred)
            VALUES (1, 'guid-1', 'Legacy article', '', 'body', 42, 0, 0);",
        )
        .unwrap();
        drop(conn);

        let db = Database::open_at(&path).unwrap();
        let article = db.get_articles(1).unwrap().remove(0);
        assert_eq!(article.author, "");
        assert_eq!(article.categories, "");

        db.upsert_article(1, "guid-1", "Updated", "", "body", 99, "Author", "rust")
            .unwrap();
        let updated = db.get_articles(1).unwrap().remove(0);
        assert_eq!(updated.author, "Author");
        assert_eq!(updated.categories, "rust");
    }
}
