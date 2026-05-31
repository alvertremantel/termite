use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Defaults {
    #[serde(default)]
    pub category: Vec<Category>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Category {
    pub name: String,
    #[serde(default)]
    pub feeds: Vec<FeedDefault>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeedDefault {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

pub fn load_defaults() -> Defaults {
    let contents = include_str!("../defaults.toml");
    toml::from_str(contents).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_load_successfully() {
        let defaults = load_defaults();
        assert!(!defaults.category.is_empty(), "should have categories");
        for cat in &defaults.category {
            assert!(!cat.name.is_empty(), "category should have a name");
            assert!(!cat.feeds.is_empty(), "category should have feeds");
            for feed in &cat.feeds {
                assert!(!feed.title.is_empty(), "feed should have a title");
                assert!(!feed.url.is_empty(), "feed should have a URL");
            }
        }
    }
}
