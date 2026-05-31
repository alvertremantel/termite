use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    Parent,
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub struct WorkspaceEntry {
    pub name: String,
    pub kind: WorkspaceEntryKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub extension: Option<String>,
    pub recent_rank: Option<usize>,
}

impl WorkspaceEntry {
    pub fn parent() -> Self {
        Self {
            name: "..".into(),
            kind: WorkspaceEntryKind::Parent,
            size: None,
            modified: None,
            extension: None,
            recent_rank: None,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, WorkspaceEntryKind::Directory)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceSortMode {
    AlphaDirsFirst,
    RecentFirst,
    ModifiedFirst,
}

impl WorkspaceSortMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::AlphaDirsFirst => Self::RecentFirst,
            Self::RecentFirst => Self::ModifiedFirst,
            Self::ModifiedFirst => Self::AlphaDirsFirst,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AlphaDirsFirst => "alpha",
            Self::RecentFirst => "recent",
            Self::ModifiedFirst => "modified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceScope {
    All,
    FilesOnly,
    DirsOnly,
}

impl WorkspaceScope {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::FilesOnly,
            Self::FilesOnly => Self::DirsOnly,
            Self::DirsOnly => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::FilesOnly => "files",
            Self::DirsOnly => "dirs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceOptions {
    pub filter: String,
    pub show_hidden: bool,
    pub sort_mode: WorkspaceSortMode,
    pub scope: WorkspaceScope,
}

impl Default for WorkspaceOptions {
    fn default() -> Self {
        Self {
            filter: String::new(),
            show_hidden: false,
            sort_mode: WorkspaceSortMode::AlphaDirsFirst,
            scope: WorkspaceScope::All,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceSummary {
    pub dirs_total: usize,
    pub files_total: usize,
    pub hidden_total: usize,
}

pub fn list_workspace_entries(
    cwd: &Path,
    opts: &WorkspaceOptions,
    recents: &[PathBuf],
) -> (Vec<WorkspaceEntry>, WorkspaceSummary) {
    let mut entries = Vec::new();
    let mut summary = WorkspaceSummary::default();
    let recent_map: HashMap<PathBuf, usize> = recents
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i))
        .collect();

    if cwd.parent().is_some()
        && matches!(opts.scope, WorkspaceScope::All | WorkspaceScope::DirsOnly)
        && opts.filter.is_empty()
    {
        entries.push(WorkspaceEntry::parent());
    }

    let Ok(read_dir) = std::fs::read_dir(cwd) else {
        return (entries, summary);
    };

    let needle = opts.filter.to_lowercase();
    for de in read_dir.flatten() {
        let name = de.file_name().to_string_lossy().into_owned();
        let hidden = name.starts_with('.');
        if hidden {
            summary.hidden_total += 1;
        }

        let ft = de.file_type().ok();
        let kind = if ft.as_ref().is_some_and(|t| t.is_dir()) {
            summary.dirs_total += 1;
            WorkspaceEntryKind::Directory
        } else if ft.as_ref().is_some_and(|t| t.is_file()) {
            summary.files_total += 1;
            WorkspaceEntryKind::File
        } else {
            continue;
        };

        if hidden && !opts.show_hidden {
            continue;
        }
        if matches!(opts.scope, WorkspaceScope::FilesOnly) && kind != WorkspaceEntryKind::File {
            continue;
        }
        if matches!(opts.scope, WorkspaceScope::DirsOnly) && kind != WorkspaceEntryKind::Directory {
            continue;
        }

        let ext = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_string());
        let hay = format!(
            "{} {}",
            name.to_lowercase(),
            ext.clone().unwrap_or_default().to_lowercase()
        );
        if !needle.is_empty() && !hay.contains(&needle) {
            continue;
        }

        let meta = de.metadata().ok();
        let path = de.path().canonicalize().unwrap_or_else(|_| de.path());
        entries.push(WorkspaceEntry {
            name,
            kind,
            size: meta.as_ref().map(|m| m.len()),
            modified: meta.and_then(|m| m.modified().ok()),
            extension: ext,
            recent_rank: recent_map.get(&path).copied(),
        });
    }

    sort_entries(&mut entries, opts.sort_mode);
    (entries, summary)
}

pub fn sort_entries(entries: &mut [WorkspaceEntry], mode: WorkspaceSortMode) {
    entries.sort_by(|a, b| {
        if a.kind == WorkspaceEntryKind::Parent {
            return std::cmp::Ordering::Less;
        }
        if b.kind == WorkspaceEntryKind::Parent {
            return std::cmp::Ordering::Greater;
        }
        match mode {
            WorkspaceSortMode::AlphaDirsFirst => kind_rank(a)
                .cmp(&kind_rank(b))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            WorkspaceSortMode::RecentFirst => a
                .recent_rank
                .unwrap_or(usize::MAX)
                .cmp(&b.recent_rank.unwrap_or(usize::MAX))
                .then_with(|| kind_rank(a).cmp(&kind_rank(b)))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            WorkspaceSortMode::ModifiedFirst => b
                .modified
                .cmp(&a.modified)
                .then_with(|| kind_rank(a).cmp(&kind_rank(b)))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
    });
}

fn kind_rank(e: &WorkspaceEntry) -> u8 {
    if e.is_dir() { 0 } else { 1 }
}

pub fn format_size(size: Option<u64>) -> String {
    let Some(n) = size else {
        return "—".into();
    };
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / 1024.0 / 1024.0)
    }
}

pub fn format_age(modified: Option<SystemTime>) -> String {
    let Some(t) = modified else {
        return "mtime —".into();
    };
    let d = SystemTime::now()
        .duration_since(t)
        .unwrap_or(Duration::ZERO);
    if d.as_secs() < 60 {
        "just now".into()
    } else if d.as_secs() < 3600 {
        format!("{}m ago", d.as_secs() / 60)
    } else if d.as_secs() < 86400 {
        format!("{}h ago", d.as_secs() / 3600)
    } else {
        format!("{}d ago", d.as_secs() / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread::sleep;

    #[test]
    fn toggles_cycle() {
        assert_eq!(
            WorkspaceSortMode::AlphaDirsFirst.cycle(),
            WorkspaceSortMode::RecentFirst
        );
        assert_eq!(WorkspaceScope::All.cycle(), WorkspaceScope::FilesOnly);
    }

    #[test]
    fn sort_recent_first_prefers_recent_files() {
        let mut entries = vec![
            WorkspaceEntry {
                name: "b.rs".into(),
                kind: WorkspaceEntryKind::File,
                size: None,
                modified: None,
                extension: Some("rs".into()),
                recent_rank: Some(0),
            },
            WorkspaceEntry {
                name: "a.rs".into(),
                kind: WorkspaceEntryKind::File,
                size: None,
                modified: None,
                extension: Some("rs".into()),
                recent_rank: None,
            },
        ];
        sort_entries(&mut entries, WorkspaceSortMode::RecentFirst);
        assert_eq!(entries[0].name, "b.rs");
    }

    #[test]
    fn list_filters_by_extension_and_hides_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("notes.txt"), "notes").unwrap();
        fs::write(dir.path().join(".secret.rs"), "hidden").unwrap();
        let opts = WorkspaceOptions {
            filter: "rs".into(),
            ..WorkspaceOptions::default()
        };
        let (entries, summary) = list_workspace_entries(dir.path(), &opts, &[]);
        assert_eq!(summary.files_total, 3);
        assert!(entries.iter().any(|e| e.name == "main.rs"));
        assert!(!entries.iter().any(|e| e.name == ".secret.rs"));
        assert!(!entries.iter().any(|e| e.name == "notes.txt"));
    }

    #[test]
    fn list_can_show_hidden_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".env"), "x=1").unwrap();

        let opts = WorkspaceOptions {
            show_hidden: true,
            ..WorkspaceOptions::default()
        };
        let (entries, summary) = list_workspace_entries(dir.path(), &opts, &[]);

        assert_eq!(summary.hidden_total, 1);
        assert!(entries.iter().any(|e| e.name == ".env"));
    }

    #[test]
    fn parent_entry_only_appears_without_filter() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "hello").unwrap();

        let filtered = WorkspaceOptions {
            filter: "file".into(),
            ..WorkspaceOptions::default()
        };
        let (filtered_entries, _) = list_workspace_entries(dir.path(), &filtered, &[]);
        assert!(
            !filtered_entries
                .iter()
                .any(|entry| entry.kind == WorkspaceEntryKind::Parent)
        );

        let dirs_only = WorkspaceOptions {
            scope: WorkspaceScope::DirsOnly,
            ..WorkspaceOptions::default()
        };
        let (dir_entries, _) = list_workspace_entries(dir.path(), &dirs_only, &[]);
        assert_eq!(
            dir_entries.first().map(|entry| entry.kind),
            Some(WorkspaceEntryKind::Parent)
        );
    }

    #[test]
    fn scope_modes_filter_files_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("notes.md"), "# Notes").unwrap();

        let files_only = WorkspaceOptions {
            scope: WorkspaceScope::FilesOnly,
            ..WorkspaceOptions::default()
        };
        let (file_entries, _) = list_workspace_entries(dir.path(), &files_only, &[]);
        assert_eq!(file_entries.len(), 1);
        assert!(
            file_entries
                .iter()
                .all(|entry| entry.kind == WorkspaceEntryKind::File)
        );

        let dirs_only = WorkspaceOptions {
            scope: WorkspaceScope::DirsOnly,
            ..WorkspaceOptions::default()
        };
        let (dir_entries, _) = list_workspace_entries(dir.path(), &dirs_only, &[]);
        assert!(
            dir_entries
                .iter()
                .any(|entry| entry.kind == WorkspaceEntryKind::Parent)
        );
        assert!(dir_entries.iter().any(|entry| entry.name == "docs"));
        assert!(!dir_entries.iter().any(|entry| entry.name == "notes.md"));
    }

    #[test]
    fn recent_and_modified_sorting_use_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join("alpha.txt");
        let newer = dir.path().join("beta.txt");
        fs::write(&older, "a").unwrap();
        sleep(Duration::from_millis(10));
        fs::write(&newer, "b").unwrap();

        let recent_paths = vec![newer.canonicalize().unwrap(), older.canonicalize().unwrap()];
        let recent_opts = WorkspaceOptions {
            sort_mode: WorkspaceSortMode::RecentFirst,
            ..WorkspaceOptions::default()
        };
        let (recent_entries, _) = list_workspace_entries(dir.path(), &recent_opts, &recent_paths);
        assert_eq!(recent_entries[1].name, "beta.txt");

        let modified_opts = WorkspaceOptions {
            sort_mode: WorkspaceSortMode::ModifiedFirst,
            ..WorkspaceOptions::default()
        };
        let (modified_entries, _) = list_workspace_entries(dir.path(), &modified_opts, &[]);
        assert_eq!(modified_entries[1].name, "beta.txt");
    }
}
