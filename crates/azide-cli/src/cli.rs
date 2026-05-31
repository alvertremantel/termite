use crate::feed_store::store_fetched_feeds;
use azide_config::{Config, ViewConfig};
use azide_feed::{ArticleContentKind, fetch_all_feeds, http_client, parse_feed_bytes};
use azide_store::Database;
use clap::{Parser, Subcommand};
use color_eyre::Result;

#[derive(Parser)]
#[command(name = "azide", about = "Azide RSS reader", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage RSS feeds
    Feeds {
        #[command(subcommand)]
        action: FeedsAction,
    },
    /// Manage articles
    Articles {
        #[command(subcommand)]
        action: ArticlesAction,
    },
    /// Manage views
    Views {
        #[command(subcommand)]
        action: ViewsAction,
    },
}

#[derive(Subcommand)]
pub enum FeedsAction {
    /// List all subscribed feeds
    List,
    /// Add a new feed by URL
    Add {
        /// RSS/Atom feed URL
        url: String,
    },
    /// Delete a feed by ID
    Delete {
        /// Feed ID
        id: i64,
    },
    /// Refresh all feeds (fetch latest articles)
    Refresh,
    /// Sanity-check an RSS feed URL
    Check {
        /// RSS/Atom feed URL to check
        url: String,
    },
}

#[derive(Subcommand)]
pub enum ArticlesAction {
    /// List articles
    List {
        /// Filter by feed ID
        #[arg(long)]
        feed: Option<i64>,
        /// Show only unread articles
        #[arg(long)]
        unread: bool,
        /// Show only saved/starred articles
        #[arg(long)]
        saved: bool,
        /// Maximum number of articles to show
        #[arg(long, default_value = "25")]
        limit: usize,
    },
    /// Read an article's content
    Read {
        /// Article ID
        id: i64,
    },
    /// Mark an article as read
    MarkRead {
        /// Article ID
        id: i64,
    },
    /// Save/star an article for later
    Save {
        /// Article ID
        id: i64,
    },
    /// Remove an article from saved/starred
    Unsave {
        /// Article ID
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum ViewsAction {
    /// List all views
    List,
    /// Create a new view
    Create {
        /// View name
        name: String,
        /// Feed URLs to include
        #[arg(long, required = true, num_args = 1..)]
        feeds: Vec<String>,
    },
    /// Delete a view by name
    Delete {
        /// View name
        name: String,
    },
}

pub async fn run_cli(command: Commands) -> Result<()> {
    match command {
        Commands::Feeds { action } => run_feeds(action).await,
        Commands::Articles { action } => run_articles(action).await,
        Commands::Views { action } => run_views(action),
    }
}

async fn run_feeds(action: FeedsAction) -> Result<()> {
    match action {
        FeedsAction::List => {
            let db = Database::open_default()?;
            let feeds = db.get_feeds()?;
            if feeds.is_empty() {
                println!("No feeds configured.");
                return Ok(());
            }
            println!("{:<6} {:<50} URL", "ID", "Title");
            println!("{}", "─".repeat(100));
            for (id, url, title) in &feeds {
                let display_title = if title.is_empty() || title == url {
                    "(untitled)"
                } else {
                    title
                };
                println!("{:<6} {:<50} {}", id, display_title, url);
            }
            println!("\n{} feed(s) total.", feeds.len());
        }
        FeedsAction::Add { url } => {
            let db = Database::open_default()?;
            let id = db.add_feed(&url, &url)?;
            println!("Added feed (id={id}). Fetching...");

            let results = fetch_all_feeds(&[(id, url.clone())]).await;
            store_fetched_feeds(&db, &results);

            if let Some(parsed) = results.first() {
                let title = parsed.title.as_deref().unwrap_or("(untitled)");
                if let Some(ref t) = parsed.title {
                    let _ = db.update_feed_title(id, t);
                }
                println!("Feed: {title}");
                println!("{} article(s) fetched.", parsed.articles.len());
            } else {
                println!("Warning: Could not fetch or parse feed at {url}");
            }
        }
        FeedsAction::Delete { id } => {
            let db = Database::open_default()?;
            db.delete_feed(id)?;
            println!("Deleted feed id={id} and its articles.");
        }
        FeedsAction::Refresh => {
            let db = Database::open_default()?;
            let feed_list = db.get_feeds()?;
            if feed_list.is_empty() {
                println!("No feeds to refresh.");
                return Ok(());
            }
            let feed_urls: Vec<(i64, String)> = feed_list
                .iter()
                .map(|(id, url, _)| (*id, url.clone()))
                .collect();
            println!("Refreshing {} feed(s)...", feed_urls.len());
            let results = fetch_all_feeds(&feed_urls).await;
            store_fetched_feeds(&db, &results);
            let total_articles: usize = results.iter().map(|r| r.articles.len()).sum();
            println!(
                "Done. Fetched {} article(s) from {} feed(s).",
                total_articles,
                results.len()
            );
        }
        FeedsAction::Check { url } => {
            println!("Checking feed: {url}");
            println!();

            let client = http_client();
            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    println!("FAIL: Could not reach URL");
                    println!("  Error: {e}");
                    return Ok(());
                }
            };

            let status = resp.status();
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            println!("HTTP Status:  {status}");
            println!("Content-Type: {content_type}");

            if !status.is_success() {
                println!("\nFAIL: Non-success HTTP status.");
                return Ok(());
            }

            let bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    println!("\nFAIL: Could not read response body: {e}");
                    return Ok(());
                }
            };

            println!("Response size: {} bytes", bytes.len());
            println!();

            let parsed = match parse_feed_bytes(0, &bytes) {
                Ok(p) => p,
                Err(e) => {
                    println!("FAIL: Not a valid RSS/Atom feed.");
                    println!("  Parse error: {e}");
                    return Ok(());
                }
            };

            let feed_title = parsed.title.as_deref().unwrap_or("(none)");
            let feed_desc = parsed.description.as_deref().unwrap_or("(none)");
            println!("OK: Valid feed parsed successfully.");
            println!();
            println!("Feed title:       {feed_title}");
            println!("Feed description: {feed_desc}");
            println!("Entry count:      {}", parsed.articles.len());

            if parsed.articles.is_empty() {
                println!("\nNote: Feed has no entries.");
                return Ok(());
            }

            // Analyze article content
            let mut has_content = 0usize;
            let mut has_summary_only = 0usize;
            let mut has_nothing = 0usize;
            let mut total_content_len = 0usize;
            let mut has_titles = 0usize;
            let mut has_links = 0usize;
            let mut has_authors = 0usize;
            let mut has_dates = 0usize;

            for entry in &parsed.articles {
                if !entry.title.is_empty() {
                    has_titles += 1;
                }
                if !entry.link.is_empty() {
                    has_links += 1;
                }
                if !entry.author.is_empty() {
                    has_authors += 1;
                }
                if entry.published != 0 {
                    has_dates += 1;
                }

                match entry.content_kind {
                    ArticleContentKind::Content => {
                        has_content += 1;
                        total_content_len += entry.content.len();
                    }
                    ArticleContentKind::Summary => {
                        has_summary_only += 1;
                        total_content_len += entry.content.len();
                    }
                    ArticleContentKind::None => {
                        has_nothing += 1;
                    }
                }
            }

            let n = parsed.articles.len();
            let avg_len = total_content_len
                .checked_div(has_content + has_summary_only)
                .unwrap_or(0);

            println!();
            println!("Article analysis ({n} entries):");
            println!("  Titles:     {has_titles}/{n}");
            println!("  Links:      {has_links}/{n}");
            println!("  Authors:    {has_authors}/{n}");
            println!("  Dates:      {has_dates}/{n}");
            println!("  Full body:  {has_content}/{n}");
            println!("  Summary:    {has_summary_only}/{n}");
            println!("  No content: {has_nothing}/{n}");
            println!("  Avg content length: {avg_len} chars");
            println!();

            if has_content > 0 {
                println!("Content type: FULL ARTICLES — feed provides complete article bodies.");
            } else if has_summary_only > 0 {
                println!(
                    "Content type: SUMMARIES ONLY — feed provides article summaries, not full text."
                );
            } else {
                println!("Content type: NO CONTENT — feed provides neither content nor summaries.");
            }

            // Show a sample entry
            if let Some(entry) = parsed.articles.first() {
                println!();
                println!("Sample entry:");
                let title = if entry.title.is_empty() {
                    "(no title)"
                } else {
                    entry.title.as_str()
                };
                println!("  Title:  {title}");
                if !entry.link.is_empty() {
                    println!("  Link:   {}", entry.link);
                }
                if !entry.author.is_empty() {
                    println!("  Author: {}", entry.author);
                }
                let content_preview = if entry.content.is_empty() {
                    "(no content)".to_string()
                } else {
                    let plain = strip_html(&entry.content);
                    let trimmed: String = plain.split_whitespace().collect::<Vec<_>>().join(" ");
                    truncate_str(&trimmed, 200)
                };
                println!("  Preview: {content_preview}");
            }
        }
    }
    Ok(())
}

async fn run_articles(action: ArticlesAction) -> Result<()> {
    let db = Database::open_default()?;

    match action {
        ArticlesAction::List {
            feed,
            unread,
            saved,
            limit,
        } => {
            if saved {
                let articles = db.get_starred_articles()?;
                if articles.is_empty() {
                    println!("No saved articles.");
                    return Ok(());
                }
                println!("{:<6} {:<60} Status", "ID", "Title");
                println!("{}", "─".repeat(80));
                for a in articles.iter().take(limit) {
                    let status = format!("{}{}", if a.read { "read" } else { "unread" }, " ★",);
                    let title = truncate_str(&a.title, 57);
                    println!("{:<6} {:<60} {}", a.id, title, status);
                }
                println!("\n{} saved article(s).", articles.len().min(limit));
            } else if unread {
                let articles = db.get_unread_articles(limit)?;
                if articles.is_empty() {
                    println!("No unread articles.");
                    return Ok(());
                }
                println!("{:<6} {:<6} {:<60} Status", "ID", "Feed", "Title");
                println!("{}", "─".repeat(86));
                for (feed_id, a) in &articles {
                    let star = if a.starred { " ★" } else { "" };
                    let title = truncate_str(&a.title, 57);
                    println!("{:<6} {:<6} {:<60} unread{star}", a.id, feed_id, title);
                }
                println!("\n{} unread article(s).", articles.len());
            } else if let Some(feed_id) = feed {
                let articles = db.get_articles(feed_id)?;
                if articles.is_empty() {
                    println!("No articles for feed id={feed_id}.");
                    return Ok(());
                }
                println!("{:<6} {:<60} Status", "ID", "Title");
                println!("{}", "─".repeat(80));
                for a in articles.iter().take(limit) {
                    let status = format!(
                        "{}{}",
                        if a.read { "read" } else { "unread" },
                        if a.starred { " ★" } else { "" },
                    );
                    let title = truncate_str(&a.title, 57);
                    println!("{:<6} {:<60} {}", a.id, title, status);
                }
                println!("\n{} article(s) shown.", articles.len().min(limit));
            } else {
                println!("Specify --feed <id>, --unread, or --saved to list articles.");
                println!("Use 'azide feeds list' to see available feed IDs.");
            }
        }
        ArticlesAction::Read { id } => {
            let article = db.get_article_by_id(id)?;
            match article {
                Some(a) => {
                    println!("{}", a.title);
                    if !a.author.is_empty() {
                        println!("By: {}", a.author);
                    }
                    if !a.link.is_empty() {
                        println!("Link: {}", a.link);
                    }
                    if !a.categories.is_empty() {
                        println!("Tags: {}", a.categories);
                    }
                    println!("{}", "─".repeat(60));

                    // Strip HTML tags for plain text output
                    let plain = strip_html(&a.content);
                    println!("{plain}");

                    // Mark as read
                    let _ = db.mark_read(id);
                }
                None => println!("Article id={id} not found."),
            }
        }
        ArticlesAction::MarkRead { id } => {
            db.mark_read(id)?;
            println!("Marked article id={id} as read.");
        }
        ArticlesAction::Save { id } => {
            db.set_starred(id, true)?;
            println!("Saved article id={id}.");
        }
        ArticlesAction::Unsave { id } => {
            db.set_starred(id, false)?;
            println!("Unsaved article id={id}.");
        }
    }
    Ok(())
}

fn run_views(action: ViewsAction) -> Result<()> {
    let mut config = Config::load()?;

    match action {
        ViewsAction::List => {
            if config.rss.views.is_empty() {
                println!("No views configured.");
                return Ok(());
            }
            for (i, view) in config.rss.views.iter().enumerate() {
                println!("{}. {} ({} feed(s))", i + 1, view.name, view.feeds.len());
                for url in &view.feeds {
                    println!("   - {url}");
                }
            }
        }
        ViewsAction::Create {
            name,
            feeds: feed_urls,
        } => {
            // Check for duplicate name
            if config.rss.views.iter().any(|v| v.name == name) {
                println!("Error: A view named '{name}' already exists.");
                return Ok(());
            }
            config.rss.views.push(ViewConfig {
                name: name.clone(),
                feeds: feed_urls.clone(),
            });
            config.save()?;
            println!("Created view '{name}' with {} feed(s).", feed_urls.len());
        }
        ViewsAction::Delete { name } => {
            let before = config.rss.views.len();
            config.rss.views.retain(|v| v.name != name);
            if config.rss.views.len() < before {
                config.save()?;
                println!("Deleted view '{name}'.");
            } else {
                println!("View '{name}' not found.");
            }
        }
    }
    Ok(())
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

fn strip_html(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut skip_content = false; // inside <script> or <style>

    while let Some(c) = chars.next() {
        if c == '<' {
            // Collect the tag name
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                tag.push(tc);
            }
            let tag_lower = tag.to_lowercase();
            let tag_name = tag_lower.split_whitespace().next().unwrap_or("");

            // Handle script/style content skipping
            if tag_name == "script" || tag_name == "style" {
                skip_content = true;
                continue;
            }
            if tag_name == "/script" || tag_name == "/style" {
                skip_content = false;
                continue;
            }

            if skip_content {
                continue;
            }

            // Insert newlines for block-level elements
            match tag_name {
                "p" | "/p" | "div" | "/div" | "br" | "br/" | "br /" | "h1" | "h2" | "h3" | "h4"
                | "h5" | "h6" | "/h1" | "/h2" | "/h3" | "/h4" | "/h5" | "/h6" | "li" | "tr"
                | "blockquote" | "/blockquote" | "hr" | "hr/"
                    if !result.ends_with('\n') =>
                {
                    result.push('\n');
                }
                _ => {}
            }
        } else if !skip_content {
            result.push(c);
        }
    }

    // Decode HTML entities using the shared decoder
    result = jones_render::html::decode_entities(&result);

    // Collapse multiple blank lines
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_temp_config_home(test: impl FnOnce() -> Result<()>) {
        let _guard = env_lock().lock().unwrap();
        let dir = TempDir::new().unwrap();
        let config_home = dir.path().join("config");
        let data_home = dir.path().join("data");

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::set_var("XDG_DATA_HOME", &data_home);
        }

        let result = test();

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("XDG_DATA_HOME");
        }

        result.unwrap();
    }

    #[test]
    fn parses_feeds_add_command() {
        let cli =
            Cli::try_parse_from(["azide", "feeds", "add", "https://example.test/rss"]).unwrap();
        match cli.command.unwrap() {
            Commands::Feeds {
                action: FeedsAction::Add { url },
            } => assert_eq!(url, "https://example.test/rss"),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_feeds_list_command() {
        let cli = Cli::try_parse_from(["azide", "feeds", "list"]).unwrap();
        assert!(matches!(
            cli.command.unwrap(),
            Commands::Feeds {
                action: FeedsAction::List
            }
        ));
    }

    #[test]
    fn parses_articles_list_unread_command() {
        let cli = Cli::try_parse_from(["azide", "articles", "list", "--unread"]).unwrap();
        match cli.command.unwrap() {
            Commands::Articles {
                action:
                    ArticlesAction::List {
                        feed,
                        unread,
                        saved,
                        limit,
                    },
            } => {
                assert_eq!(feed, None);
                assert!(unread);
                assert!(!saved);
                assert_eq!(limit, 25);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_views_create_command() {
        let cli = Cli::try_parse_from([
            "azide",
            "views",
            "create",
            "news",
            "--feeds",
            "https://example.test/a",
            "https://example.test/b",
        ])
        .unwrap();
        match cli.command.unwrap() {
            Commands::Views {
                action: ViewsAction::Create { name, feeds },
            } => {
                assert_eq!(name, "news");
                assert_eq!(feeds.len(), 2);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn run_views_create_persists_view_without_network() {
        with_temp_config_home(|| {
            run_views(ViewsAction::Create {
                name: "news".into(),
                feeds: vec![
                    "https://example.test/a.xml".into(),
                    "https://example.test/b.xml".into(),
                ],
            })?;

            let config = Config::load()?;
            assert_eq!(config.rss.views.len(), 1);
            assert_eq!(config.rss.views[0].name, "news");
            assert_eq!(config.rss.views[0].feeds.len(), 2);
            Ok(())
        });
    }

    #[test]
    fn run_views_delete_removes_existing_view_without_network() {
        with_temp_config_home(|| {
            let mut config = Config::default();
            config.rss.views.push(ViewConfig {
                name: "news".into(),
                feeds: vec!["https://example.test/a.xml".into()],
            });
            config.save()?;

            run_views(ViewsAction::Delete {
                name: "news".into(),
            })?;

            let config = Config::load()?;
            assert!(config.rss.views.is_empty());
            Ok(())
        });
    }
}
