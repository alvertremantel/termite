use std::fmt;
use std::time::Duration;

pub const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFeed {
    pub feed_id: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub articles: Vec<ParsedArticle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArticle {
    pub guid: String,
    pub title: String,
    pub link: String,
    pub content: String,
    pub content_kind: ArticleContentKind,
    pub published: i64,
    pub author: String,
    pub categories: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArticleContentKind {
    None,
    Summary,
    Content,
}

#[derive(Debug)]
pub enum FeedError {
    Http(reqwest::Error),
    Parse(feed_rs::parser::ParseFeedError),
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::Parse(error) => write!(f, "feed parse error: {error}"),
        }
    }
}

impl std::error::Error for FeedError {}

impl From<reqwest::Error> for FeedError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<feed_rs::parser::ParseFeedError> for FeedError {
    fn from(value: feed_rs::parser::ParseFeedError) -> Self {
        Self::Parse(value)
    }
}

pub fn generate_feed_tag(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    let tag = if words.len() >= 2 {
        words
            .iter()
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        title.chars().take(3).collect::<String>()
    };
    tag.chars().take(4).collect::<String>()
}

pub fn http_client() -> reqwest::Client {
    http_client_with_timeout(HTTP_TIMEOUT)
}

pub fn http_client_with_timeout(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

pub fn parse_feed_bytes(feed_id: i64, bytes: &[u8]) -> Result<ParsedFeed, FeedError> {
    let parsed = feed_rs::parser::parse(bytes)?;
    Ok(ParsedFeed {
        feed_id,
        title: parsed
            .title
            .as_ref()
            .filter(|title| !title.content.is_empty())
            .map(|title| title.content.clone()),
        description: parsed
            .description
            .as_ref()
            .map(|description| description.content.clone()),
        articles: parsed.entries.iter().map(parse_entry).collect(),
    })
}

pub async fn fetch_feed(
    client: &reqwest::Client,
    feed_id: i64,
    url: &str,
) -> Result<ParsedFeed, FeedError> {
    let bytes = client.get(url).send().await?.bytes().await?;
    parse_feed_bytes(feed_id, &bytes)
}

pub async fn fetch_all_feeds(feed_urls: &[(i64, String)]) -> Vec<ParsedFeed> {
    let client = http_client();
    let futures = feed_urls
        .iter()
        .map(|(feed_id, url)| fetch_feed(&client, *feed_id, url));
    futures::future::join_all(futures)
        .await
        .into_iter()
        .filter_map(Result::ok)
        .collect()
}

fn parse_entry(entry: &feed_rs::model::Entry) -> ParsedArticle {
    ParsedArticle {
        guid: entry.id.clone(),
        title: entry
            .title
            .as_ref()
            .map(|title| title.content.clone())
            .unwrap_or_default(),
        link: entry
            .links
            .first()
            .map(|link| link.href.clone())
            .unwrap_or_default(),
        content: entry
            .content
            .as_ref()
            .and_then(|content| content.body.clone())
            .or_else(|| {
                entry
                    .summary
                    .as_ref()
                    .map(|summary| summary.content.clone())
            })
            .unwrap_or_default(),
        content_kind: if entry
            .content
            .as_ref()
            .and_then(|content| content.body.as_ref())
            .is_some()
        {
            ArticleContentKind::Content
        } else if entry.summary.is_some() {
            ArticleContentKind::Summary
        } else {
            ArticleContentKind::None
        },
        published: entry
            .published
            .or(entry.updated)
            .map(|datetime| datetime.timestamp())
            .unwrap_or(0),
        author: entry
            .authors
            .first()
            .map(|author| author.name.clone())
            .unwrap_or_default(),
        categories: entry
            .categories
            .iter()
            .map(|category| category.term.clone())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS_FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>Example RSS</title>
    <description>Fixture RSS feed</description>
    <item>
      <guid>rss-1</guid>
      <title>RSS Article One</title>
      <link>https://example.test/rss/1</link>
      <pubDate>Mon, 12 Feb 2024 10:00:00 GMT</pubDate>
      <dc:creator>RSS Author</dc:creator>
      <category>rust</category>
      <category>news</category>
      <description><![CDATA[<p>RSS summary body</p>]]></description>
    </item>
  </channel>
</rss>"#;

    const ATOM_FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Example Atom</title>
  <subtitle>Fixture Atom feed</subtitle>
  <entry>
    <id>atom-1</id>
    <title>Atom Article One</title>
    <link href="https://example.test/atom/1" />
    <updated>2024-02-12T11:00:00Z</updated>
    <author><name>Atom Author</name></author>
    <category term="release" />
    <content type="html"><![CDATA[<p>Atom full body</p>]]></content>
  </entry>
</feed>"#;

    #[test]
    fn generate_feed_tag_uses_initials_or_prefix() {
        assert_eq!(generate_feed_tag("Example Feed"), "EF");
        assert_eq!(generate_feed_tag("news"), "new");
        assert_eq!(generate_feed_tag("Rust Weekly Journal"), "RWJ");
    }

    #[test]
    fn parse_rss_fixture() {
        let parsed = parse_feed_bytes(7, RSS_FIXTURE.as_bytes()).unwrap();

        assert_eq!(parsed.feed_id, 7);
        assert_eq!(parsed.title.as_deref(), Some("Example RSS"));
        assert_eq!(parsed.description.as_deref(), Some("Fixture RSS feed"));
        assert_eq!(parsed.articles.len(), 1);
        let article = &parsed.articles[0];
        assert_eq!(article.guid, "rss-1");
        assert_eq!(article.title, "RSS Article One");
        assert_eq!(article.link, "https://example.test/rss/1");
        assert_eq!(article.content, "<p>RSS summary body</p>");
        assert_eq!(article.content_kind, ArticleContentKind::Summary);
        assert_eq!(article.author, "RSS Author");
        assert_eq!(article.categories, "rust, news");
        assert!(article.published > 0);
    }

    #[test]
    fn parse_atom_fixture() {
        let parsed = parse_feed_bytes(9, ATOM_FIXTURE.as_bytes()).unwrap();

        assert_eq!(parsed.feed_id, 9);
        assert_eq!(parsed.title.as_deref(), Some("Example Atom"));
        assert_eq!(parsed.description.as_deref(), Some("Fixture Atom feed"));
        assert_eq!(parsed.articles.len(), 1);
        let article = &parsed.articles[0];
        assert_eq!(article.guid, "atom-1");
        assert_eq!(article.title, "Atom Article One");
        assert_eq!(article.link, "https://example.test/atom/1");
        assert_eq!(article.content, "<p>Atom full body</p>");
        assert_eq!(article.content_kind, ArticleContentKind::Content);
        assert_eq!(article.author, "Atom Author");
        assert_eq!(article.categories, "release");
        assert!(article.published > 0);
    }

    #[test]
    fn invalid_feed_bytes_fail_cleanly() {
        let error = parse_feed_bytes(1, b"not a feed").unwrap_err();
        assert!(matches!(error, FeedError::Parse(_)));
    }
}
