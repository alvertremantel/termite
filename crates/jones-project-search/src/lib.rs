use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    pub max_results: usize,
    pub skip_hidden: bool,
    pub skip_dirs: BTreeSet<String>,
    pub text_extensions: BTreeSet<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 100,
            skip_hidden: true,
            skip_dirs: ["target", "node_modules", "dist", "build"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            text_extensions: [
                "rs", "md", "txt", "toml", "json", "py", "js", "ts", "tsx", "jsx", "yaml", "yml",
                "css", "html",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

pub fn search_project(root: &Path, query: &str, max_results: usize) -> Vec<SearchResult> {
    let opts = SearchOptions {
        max_results,
        ..SearchOptions::default()
    };
    search_project_with_options(root, query, &opts)
}

pub fn search_project_with_options(
    root: &Path,
    query: &str,
    opts: &SearchOptions,
) -> Vec<SearchResult> {
    if query.trim().is_empty() || opts.max_results == 0 {
        return Vec::new();
    }
    let mut results = Vec::new();
    visit(root, query, opts, &mut results);
    results
}

fn visit(dir: &Path, query: &str, opts: &SearchOptions, results: &mut Vec<SearchResult>) {
    if results.len() >= opts.max_results {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        if results.len() >= opts.max_results {
            break;
        }
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if opts.skip_hidden && name.starts_with('.') {
            continue;
        }
        if opts.skip_dirs.contains(&name) {
            continue;
        }
        let Ok(file_type) = e.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            visit(&p, query, opts, results);
        } else if file_type.is_file()
            && is_text(&p, opts)
            && let Ok(s) = std::fs::read_to_string(&p)
        {
            for (i, l) in s.lines().enumerate() {
                if l.contains(query) {
                    results.push(SearchResult {
                        path: p.clone(),
                        line: i,
                        preview: l.trim().chars().take(160).collect(),
                    });
                    if results.len() >= opts.max_results {
                        break;
                    }
                }
            }
        }
    }
}

fn is_text(p: &Path, opts: &SearchOptions) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| opts.text_extensions.contains(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs as unix_fs;

    #[test]
    fn collects_matches() {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join("a.rs"), "fn needle() {}\n").unwrap();
        let r = search_project(d.path(), "needle", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].line, 0);
    }

    #[test]
    fn respects_result_cap() {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join("a.rs"), "needle\nneedle\nneedle\n").unwrap();
        let r = search_project(d.path(), "needle", 2);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn skips_hidden_and_build_dirs() {
        let d = tempfile::TempDir::new().unwrap();
        fs::create_dir(d.path().join("target")).unwrap();
        fs::create_dir(d.path().join("src")).unwrap();
        fs::write(d.path().join(".hidden.rs"), "needle\n").unwrap();
        fs::write(d.path().join("target").join("gen.rs"), "needle\n").unwrap();
        fs::write(d.path().join("src").join("main.rs"), "needle\n").unwrap();

        let r = search_project(d.path(), "needle", 10);
        assert_eq!(r.len(), 1);
        assert!(r[0].path.ends_with("src/main.rs"));
    }

    #[test]
    fn only_searches_allowed_text_extensions() {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join("main.rs"), "needle\n").unwrap();
        fs::write(d.path().join("archive.bin"), "needle\n").unwrap();

        let r = search_project(d.path(), "needle", 10);
        assert_eq!(r.len(), 1);
        assert!(r[0].path.ends_with("main.rs"));
    }

    #[test]
    fn empty_query_returns_no_results() {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join("main.rs"), "needle\n").unwrap();

        assert!(search_project(d.path(), "   ", 10).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks() {
        let d = tempfile::TempDir::new().unwrap();
        let source = d.path().join("source.rs");
        let link = d.path().join("link.rs");
        fs::write(&source, "needle\n").unwrap();
        unix_fs::symlink(&source, &link).unwrap();

        let r = search_project(d.path(), "needle", 10);
        assert_eq!(r.len(), 1);
        assert!(r[0].path.ends_with("source.rs"));
    }
}
