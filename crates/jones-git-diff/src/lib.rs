use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

use color_eyre::eyre::{Result, bail, eyre};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailMode {
    StatusOnly,
    Numstat,
    FullDiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotOptions {
    pub detail: DetailMode,
    pub include_untracked: bool,
    pub diff_context: u16,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            detail: DetailMode::FullDiff,
            include_untracked: true,
            diff_context: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshot {
    pub repo_root: RepoRoot,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub status: FileStatus,
    pub staged: Option<FileChange>,
    pub unstaged: Option<FileChange>,
    pub sections: Vec<DiffSection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Changed,
    Untracked,
    Conflicted,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub status: ChangeStatus,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Unknown(char),
}

impl ChangeStatus {
    fn from_xy(ch: char) -> Option<Self> {
        match ch {
            '.' | ' ' => None,
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'T' => Some(Self::TypeChanged),
            'U' => Some(Self::Unmerged),
            other => Some(Self::Unknown(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSection {
    pub source: DiffSource,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSource {
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub code: Option<i32>,
}

impl GitOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    pub fn failure(stderr: impl Into<String>, code: Option<i32>) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            success: false,
            code,
        }
    }

    fn ensure_success(self, args: &[String]) -> Result<Self> {
        if self.success {
            return Ok(self);
        }

        let command = format!("git {}", args.join(" "));
        let detail = if self.stderr.trim().is_empty() {
            "no stderr output".to_owned()
        } else {
            self.stderr.trim().to_owned()
        };

        bail!("{command} failed: {detail}");
    }
}

pub trait GitRunner {
    fn run(&self, cwd: &Path, args: &[String]) -> Result<GitOutput>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StdGitRunner;

impl GitRunner for StdGitRunner {
    fn run(&self, cwd: &Path, args: &[String]) -> Result<GitOutput> {
        let output = Command::new("git").current_dir(cwd).args(args).output()?;

        Ok(GitOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
            code: output.status.code(),
        })
    }
}

pub fn discover_repo(runner: &impl GitRunner, start: impl AsRef<Path>) -> Result<RepoRoot> {
    let args = strings(["rev-parse", "--show-toplevel"]);
    let output = runner.run(start.as_ref(), &args)?.ensure_success(&args)?;
    let root = output.stdout.trim();
    if root.is_empty() {
        bail!("git rev-parse returned an empty repository root");
    }

    Ok(RepoRoot::new(root))
}

pub fn load_snapshot(
    runner: &impl GitRunner,
    repo_root: &RepoRoot,
    options: SnapshotOptions,
) -> Result<GitSnapshot> {
    let mut status_args = strings(["status", "--porcelain=v2", "-z"]);
    status_args.push(if options.include_untracked {
        "--untracked-files=all".to_owned()
    } else {
        "--untracked-files=no".to_owned()
    });

    let status_output = runner
        .run(repo_root.path(), &status_args)?
        .ensure_success(&status_args)?;
    let entries = parse_porcelain_v2_status(&status_output.stdout)?;
    let mut files = merge_status_entries(entries);

    if matches!(options.detail, DetailMode::Numstat | DetailMode::FullDiff) {
        apply_numstat(
            &mut files,
            DiffSource::Staged,
            run_numstat(runner, repo_root, DiffSource::Staged)?,
        )?;
        apply_numstat(
            &mut files,
            DiffSource::Unstaged,
            run_numstat(runner, repo_root, DiffSource::Unstaged)?,
        )?;
    }

    if options.detail == DetailMode::FullDiff {
        apply_diff_sections(
            &mut files,
            parse_unified_diff(
                DiffSource::Staged,
                &run_unified_diff(runner, repo_root, DiffSource::Staged, options.diff_context)?,
            )?,
        );
        apply_diff_sections(
            &mut files,
            parse_unified_diff(
                DiffSource::Unstaged,
                &run_unified_diff(
                    runner,
                    repo_root,
                    DiffSource::Unstaged,
                    options.diff_context,
                )?,
            )?,
        );
    }

    Ok(GitSnapshot {
        repo_root: repo_root.clone(),
        files: files.into_values().collect(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub staged: Option<ChangeStatus>,
    pub unstaged: Option<ChangeStatus>,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumstatEntry {
    pub path: PathBuf,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

pub fn parse_porcelain_v2_status(input: &str) -> Result<Vec<StatusEntry>> {
    if input.contains('\0') {
        return parse_porcelain_v2_status_z(input);
    }

    let mut entries = Vec::new();

    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let mut chars = line.chars();
        match chars.next() {
            Some('#') => continue,
            Some('1') => entries.push(parse_ordinary_status(line)?),
            Some('2') => entries.push(parse_renamed_status(line)?),
            Some('u') => entries.push(parse_unmerged_status(line)?),
            Some('?') => {
                let path = line
                    .strip_prefix("? ")
                    .ok_or_else(|| eyre!("invalid untracked status line: {line}"))?;
                entries.push(StatusEntry {
                    path: PathBuf::from(path),
                    old_path: None,
                    staged: None,
                    unstaged: None,
                    status: FileStatus::Untracked,
                });
            }
            Some('!') => {
                let path = line
                    .strip_prefix("! ")
                    .ok_or_else(|| eyre!("invalid ignored status line: {line}"))?;
                entries.push(StatusEntry {
                    path: PathBuf::from(path),
                    old_path: None,
                    staged: None,
                    unstaged: None,
                    status: FileStatus::Ignored,
                });
            }
            Some(other) => bail!("unsupported porcelain v2 status record '{other}': {line}"),
            None => {}
        }
    }

    Ok(entries)
}

pub fn parse_numstat(input: &str) -> Result<Vec<NumstatEntry>> {
    if input.contains('\0') {
        return parse_numstat_z(input);
    }

    let mut entries = Vec::new();

    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.splitn(3, '\t');
        let added = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat added field: {line}"))
            .and_then(parse_numstat_count)?;
        let removed = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat removed field: {line}"))
            .and_then(parse_numstat_count)?;
        let path = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat path field: {line}"))?;

        entries.push(NumstatEntry {
            path: PathBuf::from(path),
            added,
            removed,
        });
    }

    Ok(entries)
}

fn parse_porcelain_v2_status_z(input: &str) -> Result<Vec<StatusEntry>> {
    let mut entries = Vec::new();
    let mut records = input.split_terminator('\0');

    while let Some(record) = records.next() {
        if record.trim().is_empty() {
            continue;
        }

        let mut chars = record.chars();
        match chars.next() {
            Some('#') => continue,
            Some('1') => entries.push(parse_ordinary_status(record)?),
            Some('2') => {
                let mut entry = parse_renamed_status_z(record)?;
                entry.old_path = Some(PathBuf::from(records.next().ok_or_else(|| {
                    eyre!("renamed/copied status record missing original path: {record}")
                })?));
                entries.push(entry);
            }
            Some('u') => entries.push(parse_unmerged_status(record)?),
            Some('?') => {
                let path = record
                    .strip_prefix("? ")
                    .ok_or_else(|| eyre!("invalid untracked status record: {record}"))?;
                entries.push(StatusEntry {
                    path: PathBuf::from(path),
                    old_path: None,
                    staged: None,
                    unstaged: None,
                    status: FileStatus::Untracked,
                });
            }
            Some('!') => {
                let path = record
                    .strip_prefix("! ")
                    .ok_or_else(|| eyre!("invalid ignored status record: {record}"))?;
                entries.push(StatusEntry {
                    path: PathBuf::from(path),
                    old_path: None,
                    staged: None,
                    unstaged: None,
                    status: FileStatus::Ignored,
                });
            }
            Some(other) => bail!("unsupported porcelain v2 status record '{other}': {record}"),
            None => {}
        }
    }

    Ok(entries)
}

fn parse_numstat_z(input: &str) -> Result<Vec<NumstatEntry>> {
    let mut entries = Vec::new();
    let mut records = input.split_terminator('\0');

    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }

        let mut fields = record.splitn(3, '\t');
        let added = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat added field: {record}"))
            .and_then(parse_numstat_count)?;
        let removed = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat removed field: {record}"))
            .and_then(parse_numstat_count)?;
        let path = fields
            .next()
            .ok_or_else(|| eyre!("missing numstat path field: {record}"))?;
        let path = if path.is_empty() {
            let _old_path = records
                .next()
                .ok_or_else(|| eyre!("missing renamed/copied numstat old path: {record}"))?;
            records
                .next()
                .ok_or_else(|| eyre!("missing renamed/copied numstat new path: {record}"))?
        } else {
            path
        };

        entries.push(NumstatEntry {
            path: PathBuf::from(path),
            added,
            removed,
        });
    }

    Ok(entries)
}

pub fn parse_unified_diff(source: DiffSource, input: &str) -> Result<Vec<DiffSection>> {
    let mut sections = Vec::new();
    let mut section: Option<DiffSection> = None;
    let mut old_line = 0;
    let mut new_line = 0;

    for line in input.lines() {
        if line.starts_with("diff --git ") {
            if let Some(section) = section.take() {
                sections.push(section);
            }

            let (old_path, path) = parse_diff_git_paths(line)?;
            section = Some(DiffSection {
                source,
                path,
                old_path,
                hunks: Vec::new(),
            });
            continue;
        }

        let Some(current) = section.as_mut() else {
            continue;
        };

        if line.starts_with("rename from ") || line.starts_with("copy from ") {
            let path = line
                .split_once(' ')
                .and_then(|(_, rest)| rest.split_once(' '))
                .map(|(_, path)| path)
                .unwrap_or_default();
            current.old_path = Some(PathBuf::from(parse_git_path_token(path)?.0));
            continue;
        }

        if line.starts_with("rename to ") || line.starts_with("copy to ") {
            let path = line
                .split_once(' ')
                .and_then(|(_, rest)| rest.split_once(' '))
                .map(|(_, path)| path)
                .unwrap_or_default();
            current.path = PathBuf::from(parse_git_path_token(path)?.0);
            continue;
        }

        if line.starts_with("@@ ") {
            let hunk = parse_hunk_header(line)?;
            old_line = hunk.old_start;
            new_line = hunk.new_start;
            current.hunks.push(hunk);
            continue;
        }

        let Some(hunk) = current.hunks.last_mut() else {
            continue;
        };

        if let Some(content) = line.strip_prefix(' ') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(old_line),
                new_lineno: Some(new_line),
                content: content.to_owned(),
            });
            old_line += 1;
            new_line += 1;
        } else if let Some(content) = line.strip_prefix('+') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Addition,
                old_lineno: None,
                new_lineno: Some(new_line),
                content: content.to_owned(),
            });
            new_line += 1;
        } else if let Some(content) = line.strip_prefix('-') {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::Deletion,
                old_lineno: Some(old_line),
                new_lineno: None,
                content: content.to_owned(),
            });
            old_line += 1;
        } else if let Some(content) = line.strip_prefix("\\ ") {
            hunk.lines.push(DiffLine {
                kind: DiffLineKind::NoNewline,
                old_lineno: None,
                new_lineno: None,
                content: content.to_owned(),
            });
        }
    }

    if let Some(section) = section {
        sections.push(section);
    }

    Ok(sections)
}

fn strings<const N: usize>(items: [&str; N]) -> Vec<String> {
    items.into_iter().map(str::to_owned).collect()
}

fn run_numstat(
    runner: &impl GitRunner,
    repo_root: &RepoRoot,
    source: DiffSource,
) -> Result<String> {
    let args = match source {
        DiffSource::Staged => strings(["diff", "--cached", "--numstat", "-z"]),
        DiffSource::Unstaged => strings(["diff", "--numstat", "-z"]),
    };
    Ok(runner
        .run(repo_root.path(), &args)?
        .ensure_success(&args)?
        .stdout)
}

fn run_unified_diff(
    runner: &impl GitRunner,
    repo_root: &RepoRoot,
    source: DiffSource,
    context: u16,
) -> Result<String> {
    let context_arg = format!("--unified={context}");
    let args = match source {
        DiffSource::Staged => vec!["diff".to_owned(), "--cached".to_owned(), context_arg],
        DiffSource::Unstaged => vec!["diff".to_owned(), context_arg],
    };
    Ok(runner
        .run(repo_root.path(), &args)?
        .ensure_success(&args)?
        .stdout)
}

fn parse_ordinary_status(line: &str) -> Result<StatusEntry> {
    let fields: Vec<&str> = line.splitn(9, ' ').collect();
    if fields.len() != 9 {
        bail!("invalid ordinary status line: {line}");
    }

    let (staged, unstaged) = parse_xy(fields[1])?;
    Ok(StatusEntry {
        path: PathBuf::from(fields[8]),
        old_path: None,
        staged,
        unstaged,
        status: FileStatus::Changed,
    })
}

fn parse_renamed_status(line: &str) -> Result<StatusEntry> {
    let fields: Vec<&str> = line.splitn(10, ' ').collect();
    if fields.len() != 10 {
        bail!("invalid renamed/copied status line: {line}");
    }

    let (staged, unstaged) = parse_xy(fields[1])?;
    let (path, old_path) = fields[9]
        .split_once('\t')
        .ok_or_else(|| eyre!("renamed/copied status line missing original path: {line}"))?;

    Ok(StatusEntry {
        path: PathBuf::from(path),
        old_path: Some(PathBuf::from(old_path)),
        staged,
        unstaged,
        status: FileStatus::Changed,
    })
}

fn parse_renamed_status_z(record: &str) -> Result<StatusEntry> {
    let fields: Vec<&str> = record.splitn(10, ' ').collect();
    if fields.len() != 10 {
        bail!("invalid renamed/copied status record: {record}");
    }

    let (staged, unstaged) = parse_xy(fields[1])?;
    Ok(StatusEntry {
        path: PathBuf::from(fields[9]),
        old_path: None,
        staged,
        unstaged,
        status: FileStatus::Changed,
    })
}

fn parse_unmerged_status(line: &str) -> Result<StatusEntry> {
    let fields: Vec<&str> = line.splitn(11, ' ').collect();
    if fields.len() != 11 {
        bail!("invalid unmerged status line: {line}");
    }

    let (staged, unstaged) = parse_xy(fields[1])?;
    Ok(StatusEntry {
        path: PathBuf::from(fields[10]),
        old_path: None,
        staged: staged.or(Some(ChangeStatus::Unmerged)),
        unstaged: unstaged.or(Some(ChangeStatus::Unmerged)),
        status: FileStatus::Conflicted,
    })
}

fn parse_xy(field: &str) -> Result<(Option<ChangeStatus>, Option<ChangeStatus>)> {
    let mut chars = field.chars();
    let x = chars
        .next()
        .ok_or_else(|| eyre!("missing staged status in XY field"))?;
    let y = chars
        .next()
        .ok_or_else(|| eyre!("missing unstaged status in XY field"))?;
    if chars.next().is_some() {
        bail!("invalid XY field: {field}");
    }

    Ok((ChangeStatus::from_xy(x), ChangeStatus::from_xy(y)))
}

fn parse_numstat_count(input: &str) -> Result<Option<u32>> {
    if input == "-" {
        return Ok(None);
    }

    input
        .parse()
        .map(Some)
        .map_err(|error| eyre!("invalid numstat count '{input}': {error}"))
}

fn parse_diff_git_paths(line: &str) -> Result<(Option<PathBuf>, PathBuf)> {
    let rest = line
        .strip_prefix("diff --git ")
        .ok_or_else(|| eyre!("invalid diff git header: {line}"))?;
    let (old, rest) = parse_git_path_token(rest)?;
    let (new, rest) = parse_git_path_token(rest.trim_start())?;
    if !rest.trim().is_empty() {
        bail!("diff git header has trailing path data: {line}");
    }
    let old = old
        .strip_prefix("a/")
        .ok_or_else(|| eyre!("diff git header missing old path: {line}"))?;
    let new = new
        .strip_prefix("b/")
        .ok_or_else(|| eyre!("diff git header missing new path: {line}"))?;

    let old_path = PathBuf::from(old);
    let new_path = PathBuf::from(new);
    let old_path = (old_path != new_path).then_some(old_path);

    Ok((old_path, new_path))
}

fn parse_git_path_token(input: &str) -> Result<(String, &str)> {
    let Some(rest) = input.strip_prefix('"') else {
        return match input.split_once(' ') {
            Some((path, rest)) => Ok((path.to_owned(), rest)),
            None => Ok((input.to_owned(), "")),
        };
    };

    let mut output = String::new();
    let mut chars = rest.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '"' => return Ok((output, &rest[idx + ch.len_utf8()..])),
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    bail!("unterminated escape in quoted git path: {input}");
                };
                match escaped {
                    'a' => output.push('\x07'),
                    'b' => output.push('\x08'),
                    'f' => output.push('\x0c'),
                    'n' => output.push('\n'),
                    'r' => output.push('\r'),
                    't' => output.push('\t'),
                    'v' => output.push('\x0b'),
                    '\\' => output.push('\\'),
                    '"' => output.push('"'),
                    '0'..='7' => {
                        let mut value = escaped.to_digit(8).unwrap();
                        for _ in 0..2 {
                            let Some((_, next)) = chars.peek().copied() else {
                                break;
                            };
                            if !matches!(next, '0'..='7') {
                                break;
                            }
                            chars.next();
                            value = value * 8 + next.to_digit(8).unwrap();
                        }
                        output.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
                    }
                    other => output.push(other),
                }
            }
            other => output.push(other),
        }
    }

    bail!("unterminated quoted git path: {input}")
}

fn parse_hunk_header(line: &str) -> Result<DiffHunk> {
    let after_open = line
        .strip_prefix("@@ -")
        .ok_or_else(|| eyre!("invalid hunk header: {line}"))?;
    let (old_range, rest) = after_open
        .split_once(" +")
        .ok_or_else(|| eyre!("hunk header missing new range: {line}"))?;
    let (new_range, header) = rest
        .split_once(" @@")
        .ok_or_else(|| eyre!("hunk header missing terminator: {line}"))?;
    let (old_start, old_count) = parse_hunk_range(old_range)?;
    let (new_start, new_count) = parse_hunk_range(new_range)?;

    Ok(DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        header: header.trim_start().to_owned(),
        lines: Vec::new(),
    })
}

fn parse_hunk_range(input: &str) -> Result<(u32, u32)> {
    let (start, count) = input.split_once(',').unwrap_or((input, "1"));
    let start = start
        .parse()
        .map_err(|error| eyre!("invalid hunk range start '{start}': {error}"))?;
    let count = count
        .parse()
        .map_err(|error| eyre!("invalid hunk range count '{count}': {error}"))?;
    Ok((start, count))
}

fn merge_status_entries(entries: Vec<StatusEntry>) -> BTreeMap<PathBuf, ChangedFile> {
    let mut files = BTreeMap::new();

    for entry in entries {
        files.insert(
            entry.path.clone(),
            ChangedFile {
                path: entry.path,
                old_path: entry.old_path,
                status: entry.status,
                staged: entry.staged.map(empty_change),
                unstaged: entry.unstaged.map(empty_change),
                sections: Vec::new(),
            },
        );
    }

    files
}

fn empty_change(status: ChangeStatus) -> FileChange {
    FileChange {
        status,
        added: None,
        removed: None,
    }
}

fn apply_numstat(
    files: &mut BTreeMap<PathBuf, ChangedFile>,
    source: DiffSource,
    numstat: String,
) -> Result<()> {
    for entry in parse_numstat(&numstat)? {
        let file = files
            .entry(entry.path.clone())
            .or_insert_with(|| changed_file_from_path(entry.path.clone()));
        let change = match source {
            DiffSource::Staged => file
                .staged
                .get_or_insert_with(|| empty_change(ChangeStatus::Modified)),
            DiffSource::Unstaged => file
                .unstaged
                .get_or_insert_with(|| empty_change(ChangeStatus::Modified)),
        };
        change.added = entry.added;
        change.removed = entry.removed;
    }

    Ok(())
}

fn apply_diff_sections(files: &mut BTreeMap<PathBuf, ChangedFile>, sections: Vec<DiffSection>) {
    for section in sections {
        let file = files
            .entry(section.path.clone())
            .or_insert_with(|| changed_file_from_path(section.path.clone()));
        if file.old_path.is_none() {
            file.old_path = section.old_path.clone();
        }
        file.sections.push(section);
    }
}

fn changed_file_from_path(path: PathBuf) -> ChangedFile {
    ChangedFile {
        path,
        old_path: None,
        status: FileStatus::Changed,
        staged: None,
        unstaged: None,
        sections: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};

    #[test]
    fn parses_porcelain_v2_status() {
        let input = "\
# branch.oid abc123
1 .M N... 100644 100644 100644 aaaaa bbbbb src/lib.rs
1 A. N... 000000 100644 100644 00000 ccccc added.rs
2 R. N... 100644 100644 100644 ddddd eeeee R100 new.rs\told.rs
u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.rs
? scratch.txt
";

        let entries = parse_porcelain_v2_status(input).unwrap();

        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].path, PathBuf::from("src/lib.rs"));
        assert_eq!(entries[0].staged, None);
        assert_eq!(entries[0].unstaged, Some(ChangeStatus::Modified));
        assert_eq!(entries[1].staged, Some(ChangeStatus::Added));
        assert_eq!(entries[2].staged, Some(ChangeStatus::Renamed));
        assert_eq!(entries[2].old_path, Some(PathBuf::from("old.rs")));
        assert_eq!(entries[3].status, FileStatus::Conflicted);
        assert_eq!(entries[4].status, FileStatus::Untracked);
    }

    #[test]
    fn parses_numstat_text_and_binary_entries() {
        let entries = parse_numstat("12\t3\tsrc/lib.rs\n-\t-\tassets/logo.png\n").unwrap();

        assert_eq!(
            entries,
            vec![
                NumstatEntry {
                    path: PathBuf::from("src/lib.rs"),
                    added: Some(12),
                    removed: Some(3),
                },
                NumstatEntry {
                    path: PathBuf::from("assets/logo.png"),
                    added: None,
                    removed: None,
                },
            ]
        );
    }

    #[test]
    fn parses_z_numstat_rename_to_destination_path() {
        let entries = parse_numstat("12\t3\t\0old name.rs\0new name.rs\0").unwrap();

        assert_eq!(
            entries,
            vec![NumstatEntry {
                path: PathBuf::from("new name.rs"),
                added: Some(12),
                removed: Some(3),
            }]
        );
    }

    #[test]
    fn parses_z_porcelain_rename_with_special_paths() {
        let input =
            "2 R. N... 100644 100644 100644 ddddd eeeee R100 new\tname.rs\0old \"name\".rs\0";

        let entries = parse_porcelain_v2_status(input).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("new\tname.rs"));
        assert_eq!(entries[0].old_path, Some(PathBuf::from("old \"name\".rs")));
        assert_eq!(entries[0].staged, Some(ChangeStatus::Renamed));
    }

    #[test]
    fn parses_unified_diff_sections_and_line_numbers() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@ fn main()
 keep
-old
+new
+extra
\\ No newline at end of file
";

        let sections = parse_unified_diff(DiffSource::Unstaged, input).unwrap();

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].path, PathBuf::from("src/lib.rs"));
        assert_eq!(sections[0].hunks[0].old_start, 1);
        assert_eq!(sections[0].hunks[0].new_count, 4);
        assert_eq!(sections[0].hunks[0].header, "fn main()");
        assert_eq!(
            sections[0].hunks[0].lines,
            vec![
                DiffLine {
                    kind: DiffLineKind::Context,
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                    content: "keep".to_owned(),
                },
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_lineno: Some(2),
                    new_lineno: None,
                    content: "old".to_owned(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_lineno: None,
                    new_lineno: Some(2),
                    content: "new".to_owned(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_lineno: None,
                    new_lineno: Some(3),
                    content: "extra".to_owned(),
                },
                DiffLine {
                    kind: DiffLineKind::NoNewline,
                    old_lineno: None,
                    new_lineno: None,
                    content: "No newline at end of file".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn parses_hunk_lines_that_look_like_file_headers() {
        let input = "\
diff --git a/notes.txt b/notes.txt
--- a/notes.txt
+++ b/notes.txt
@@ -1,2 +1,2 @@
--- deleted heading
+++ added heading
";

        let sections = parse_unified_diff(DiffSource::Unstaged, input).unwrap();

        assert_eq!(
            sections[0].hunks[0].lines,
            vec![
                DiffLine {
                    kind: DiffLineKind::Deletion,
                    old_lineno: Some(1),
                    new_lineno: None,
                    content: "-- deleted heading".to_owned(),
                },
                DiffLine {
                    kind: DiffLineKind::Addition,
                    old_lineno: None,
                    new_lineno: Some(1),
                    content: "++ added heading".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn parses_quoted_diff_paths() {
        let input = "\
diff --git \"a/tab\\tname.rs\" \"b/quote\\\"name.rs\"
rename from \"tab\\tname.rs\"
rename to \"quote\\\"name.rs\"
@@ -1 +1 @@
-old
+new
";

        let sections = parse_unified_diff(DiffSource::Unstaged, input).unwrap();

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].old_path, Some(PathBuf::from("tab\tname.rs")));
        assert_eq!(sections[0].path, PathBuf::from("quote\"name.rs"));
    }

    #[test]
    fn load_snapshot_merges_staged_unstaged_and_untracked_files() {
        let runner = FakeRunner::new([
            (
                vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
                "\
1 MM N... 100644 100644 100644 aaaaa bbbbb both.rs
1 A. N... 000000 100644 100644 00000 ccccc staged.rs
? notes.txt
",
            ),
            (
                vec!["diff", "--cached", "--numstat", "-z"],
                "5\t1\tboth.rs\n2\t0\tstaged.rs\n",
            ),
            (vec!["diff", "--numstat", "-z"], "3\t4\tboth.rs\n"),
            (
                vec!["diff", "--cached", "--unified=3"],
                "\
diff --git a/both.rs b/both.rs
@@ -1 +1 @@
-old
+staged
diff --git a/staged.rs b/staged.rs
@@ -0,0 +1 @@
+new
",
            ),
            (
                vec!["diff", "--unified=3"],
                "\
diff --git a/both.rs b/both.rs
@@ -1 +1 @@
-staged
+worktree
",
            ),
        ]);

        let snapshot = load_snapshot(
            &runner,
            &RepoRoot::new("/repo"),
            SnapshotOptions {
                detail: DetailMode::FullDiff,
                include_untracked: true,
                diff_context: 3,
            },
        )
        .unwrap();

        let both = snapshot
            .files
            .iter()
            .find(|file| file.path == Path::new("both.rs"))
            .unwrap();
        assert_eq!(both.staged.as_ref().unwrap().added, Some(5));
        assert_eq!(both.unstaged.as_ref().unwrap().removed, Some(4));
        assert_eq!(both.sections.len(), 2);
        assert!(
            both.sections
                .iter()
                .any(|section| section.source == DiffSource::Staged)
        );
        assert!(
            both.sections
                .iter()
                .any(|section| section.source == DiffSource::Unstaged)
        );

        let untracked = snapshot
            .files
            .iter()
            .find(|file| file.path == Path::new("notes.txt"))
            .unwrap();
        assert_eq!(untracked.status, FileStatus::Untracked);
        assert!(untracked.sections.is_empty());
    }

    #[test]
    fn discover_repo_uses_runner_cwd() {
        let runner = FakeRunner::new([(vec!["rev-parse", "--show-toplevel"], "/repo\n")]);

        let root = discover_repo(&runner, "/repo/subdir").unwrap();

        assert_eq!(root.path(), Path::new("/repo"));
        assert_eq!(runner.cwd_log.borrow()[0], PathBuf::from("/repo/subdir"));
    }

    struct FakeRunner {
        outputs: HashMap<Vec<String>, GitOutput>,
        cwd_log: RefCell<Vec<PathBuf>>,
    }

    impl FakeRunner {
        fn new<const N: usize>(outputs: [(Vec<&'static str>, &'static str); N]) -> Self {
            Self {
                outputs: outputs
                    .into_iter()
                    .map(|(args, stdout)| {
                        (
                            args.into_iter().map(str::to_owned).collect(),
                            GitOutput::success(stdout),
                        )
                    })
                    .collect(),
                cwd_log: RefCell::new(Vec::new()),
            }
        }
    }

    impl GitRunner for FakeRunner {
        fn run(&self, cwd: &Path, args: &[String]) -> Result<GitOutput> {
            self.cwd_log.borrow_mut().push(cwd.to_path_buf());
            self.outputs
                .get(args)
                .cloned()
                .ok_or_else(|| eyre!("unexpected git args: {args:?}"))
        }
    }
}
