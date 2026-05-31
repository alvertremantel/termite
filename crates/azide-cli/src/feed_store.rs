//! Thin persistence helpers moved from azide-app so azide-cli does
//! not depend on any TUI crate.

use azide_feed::ParsedFeed;
use azide_store::Database;

pub(crate) fn store_parsed_feed(db: &Database, feed: &ParsedFeed) {
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
