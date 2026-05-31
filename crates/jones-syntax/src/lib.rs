use jones_theme as theme;
use ratatui::style::{Modifier, Style};

/// A highlighter that produces styled spans for a single line of text.
pub enum Highlighter {
    Plain,
    Markdown,
    Json,
    Toml,
    Rust,
    Python,
    Shell,
}

impl Highlighter {
    /// Choose a highlighter based on the file extension.
    pub fn for_path(path: Option<&std::path::Path>) -> Self {
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext.to_lowercase().as_str() {
            "md" | "markdown" => Highlighter::Markdown,
            "json" => Highlighter::Json,
            "toml" => Highlighter::Toml,
            "rs" => Highlighter::Rust,
            "py" => Highlighter::Python,
            "sh" | "bash" => Highlighter::Shell,
            _ => Highlighter::Plain,
        }
    }

    /// Choose a highlighter based on path, then fall back to a first-line shebang.
    pub fn for_path_or_shebang(path: Option<&std::path::Path>, first_line: Option<&str>) -> Self {
        let highlighter = Self::for_path(path);
        if !matches!(highlighter, Highlighter::Plain) {
            return highlighter;
        }
        if first_line.is_some_and(is_shell_shebang) {
            Highlighter::Shell
        } else {
            Highlighter::Plain
        }
    }

    /// Highlight a single line and return styled spans.
    pub fn highlight_line(&self, line: &str) -> Vec<(Style, String)> {
        match self {
            Highlighter::Plain => plain_highlight_line(line),
            Highlighter::Markdown => markdown_highlight_line(line),
            Highlighter::Json => json_highlight_line(line),
            Highlighter::Toml => toml_highlight_line(line),
            Highlighter::Rust => rust_highlight_line(line),
            Highlighter::Python => python_highlight_line(line),
            Highlighter::Shell => shell_highlight_line(line),
        }
    }
}

fn plain_highlight_line(line: &str) -> Vec<(Style, String)> {
    vec![(Style::default().fg(theme::TEXT_PRIMARY), line.to_string())]
}

fn heading_style(level: usize) -> Style {
    let color = match level {
        1 => theme::HEADING_H1,
        2 => theme::HEADING_H2,
        3 => theme::HEADING_H3,
        4 => theme::HEADING_H4,
        5 => theme::HEADING_H5,
        _ => theme::HEADING_H6,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn bold_style() -> Style {
    Style::default()
        .fg(theme::TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD)
}

fn italic_style() -> Style {
    Style::default()
        .fg(theme::TEXT_PRIMARY)
        .add_modifier(Modifier::ITALIC)
}

fn code_style() -> Style {
    Style::default().fg(theme::CODE_FG).bg(theme::CODE_BG)
}

fn link_style() -> Style {
    Style::default()
        .fg(theme::LINK)
        .add_modifier(Modifier::UNDERLINED)
}

fn blockquote_style() -> Style {
    Style::default().fg(theme::BLOCKQUOTE)
}

fn rule_style() -> Style {
    Style::default().fg(theme::RULE)
}

fn list_marker_style() -> Style {
    Style::default().fg(theme::LIST_BULLET)
}

fn default_style() -> Style {
    Style::default().fg(theme::TEXT_PRIMARY)
}

fn markdown_highlight_line(line: &str) -> Vec<(Style, String)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return vec![(default_style(), line.to_string())];
    }
    if is_horizontal_rule(trimmed) {
        return vec![(rule_style(), line.to_string())];
    }
    if let Some(level) = heading_level(trimmed) {
        return vec![(heading_style(level), line.to_string())];
    }
    if trimmed.starts_with('>') {
        return vec![(blockquote_style(), line.to_string())];
    }
    if let Some((marker_end, _indent)) = list_marker_end(line) {
        let mut spans = Vec::new();
        spans.push((list_marker_style(), line[..marker_end].to_string()));
        let rest = &line[marker_end..];
        if !rest.is_empty() {
            spans.extend(highlight_inline(rest));
        }
        return spans;
    }
    highlight_inline(line)
}

fn highlight_inline(text: &str) -> Vec<(Style, String)> {
    let mut spans: Vec<(Style, String)> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut plain_start = 0;

    while i < len {
        if chars[i] == '`'
            && let Some(end) = find_closing_backtick(&chars, i + 1)
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let code: String = chars[i..=end].iter().collect();
            spans.push((code_style(), code));
            i = end + 1;
            plain_start = i;
            continue;
        }
        if i + 1 < len
            && chars[i] == '*'
            && chars[i + 1] == '*'
            && let Some(end) = find_double_marker(&chars, i + 2, '*')
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let bold: String = chars[i..=end + 1].iter().collect();
            spans.push((bold_style(), bold));
            i = end + 2;
            plain_start = i;
            continue;
        }
        if i + 1 < len
            && chars[i] == '_'
            && chars[i + 1] == '_'
            && let Some(end) = find_double_marker(&chars, i + 2, '_')
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let bold: String = chars[i..=end + 1].iter().collect();
            spans.push((bold_style(), bold));
            i = end + 2;
            plain_start = i;
            continue;
        }
        if chars[i] == '*'
            && (i + 1 < len && chars[i + 1] != '*')
            && let Some(end) = find_single_marker(&chars, i + 1, '*')
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let italic: String = chars[i..=end].iter().collect();
            spans.push((italic_style(), italic));
            i = end + 1;
            plain_start = i;
            continue;
        }
        if chars[i] == '_'
            && (i + 1 < len && chars[i + 1] != '_')
            && let Some(end) = find_single_marker(&chars, i + 1, '_')
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let italic: String = chars[i..=end].iter().collect();
            spans.push((italic_style(), italic));
            i = end + 1;
            plain_start = i;
            continue;
        }
        if chars[i] == '['
            && let Some((_bracket_end, paren_end)) = find_link(&chars, i)
        {
            flush_plain(plain_start, i, &chars, &mut spans);
            let link: String = chars[i..=paren_end].iter().collect();
            spans.push((link_style(), link));
            i = paren_end + 1;
            plain_start = i;
            continue;
        }
        i += 1;
    }

    if plain_start < len {
        let remainder: String = chars[plain_start..].iter().collect();
        if !remainder.is_empty() {
            spans.push((default_style(), remainder));
        }
    }

    if spans.is_empty() {
        spans.push((default_style(), text.to_string()));
    }

    spans
}

fn flush_plain(start: usize, end: usize, chars: &[char], spans: &mut Vec<(Style, String)>) {
    if start < end {
        let s: String = chars[start..end].iter().collect();
        if !s.is_empty() {
            spans.push((default_style(), s));
        }
    }
}

fn find_closing_backtick(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == '`')
}

fn find_double_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    let len = chars.len();
    (from..len.saturating_sub(1)).find(|&j| chars[j] == marker && chars[j + 1] == marker)
}

fn find_single_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == marker)
}

fn find_link(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let len = chars.len();
    let bracket_end = chars[(start + 1)..len]
        .iter()
        .position(|&ch| ch == ']' || ch == '\n')
        .map(|pos| pos + start + 1)
        .filter(|&pos| chars[pos] == ']')?;
    if bracket_end + 1 >= len || chars[bracket_end + 1] != '(' {
        return None;
    }
    chars[(bracket_end + 2)..len]
        .iter()
        .position(|&ch| ch == ')' || ch == '\n')
        .map(|pos| pos + bracket_end + 2)
        .filter(|&pos| chars[pos] == ')')
        .map(|paren_end| (bracket_end, paren_end))
}

fn heading_level(trimmed: &str) -> Option<usize> {
    let mut level = 0;
    let mut found_space = false;
    for ch in trimmed.chars() {
        if ch == '#' && !found_space {
            level += 1;
        } else if ch == ' ' && level > 0 {
            found_space = true;
            break;
        } else {
            return None;
        }
    }
    if found_space && (1..=6).contains(&level) {
        Some(level)
    } else {
        None
    }
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let filtered: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if filtered.len() < 3 {
        return false;
    }
    let first = filtered[0];
    (first == '-' || first == '*' || first == '_') && filtered.iter().all(|&c| c == first)
}

fn list_marker_end(line: &str) -> Option<(usize, usize)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    if rest.len() >= 2 {
        let first = rest.as_bytes()[0];
        if (first == b'-' || first == b'*' || first == b'+') && rest.as_bytes()[1] == b' ' {
            return Some((indent + 2, indent));
        }
    }
    let mut digits = 0;
    for (j, b) in rest.bytes().enumerate() {
        if b.is_ascii_digit() {
            digits += 1;
        } else if b == b'.' && digits > 0 && j + 1 < rest.len() && rest.as_bytes()[j + 1] == b' ' {
            return Some((indent + j + 2, indent));
        } else {
            break;
        }
    }
    None
}

fn json_highlight_line(line: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let punct_style = Style::default().fg(theme::TEXT_DIM);
    let string_style = Style::default().fg(theme::ACCENT_GREEN);
    let number_style = Style::default().fg(theme::ACCENT_ORANGE);
    let bool_null_style = Style::default().fg(theme::ACCENT_MAGENTA);

    while i < len {
        let c = chars[i];
        if c.is_whitespace() {
            let start = i;
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
            spans.push((
                Style::default().fg(theme::TEXT_PRIMARY),
                chars[start..i].iter().collect(),
            ));
            continue;
        }
        if c == '"' {
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '"' && chars[i - 1] != '\\' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push((string_style, chars[start..i].iter().collect()));
            continue;
        }
        if c.is_ascii_digit() || (c == '-' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            if c == '-' {
                i += 1;
            }
            while i < len
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || chars[i] == '+'
                    || chars[i] == '-')
            {
                i += 1;
            }
            spans.push((number_style, chars[start..i].iter().collect()));
            continue;
        }
        if chars[i..].starts_with(&['t', 'r', 'u', 'e']) {
            spans.push((bool_null_style, "true".to_string()));
            i += 4;
            continue;
        }
        if chars[i..].starts_with(&['f', 'a', 'l', 's', 'e']) {
            spans.push((bool_null_style, "false".to_string()));
            i += 5;
            continue;
        }
        if chars[i..].starts_with(&['n', 'u', 'l', 'l']) {
            spans.push((bool_null_style, "null".to_string()));
            i += 4;
            continue;
        }
        spans.push((punct_style, c.to_string()));
        i += 1;
    }

    if spans.is_empty() {
        spans.push((Style::default().fg(theme::TEXT_PRIMARY), line.to_string()));
    }
    spans
}

fn toml_highlight_line(line: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let comment_style = Style::default().fg(theme::TEXT_DIM);
    let key_style = Style::default().fg(theme::ACCENT_BLUE);
    let string_style = Style::default().fg(theme::ACCENT_GREEN);
    let number_style = Style::default().fg(theme::ACCENT_ORANGE);
    let bool_style = Style::default().fg(theme::ACCENT_MAGENTA);
    let date_style = Style::default().fg(theme::ACCENT_CYAN_RGB);
    let punct_style = Style::default().fg(theme::TEXT_DIM);

    if let Some(pos) = chars.iter().position(|&c| c == '#') {
        if pos > 0 {
            spans.extend(toml_highlight_line_no_comment(
                &chars[..pos].iter().collect::<String>(),
            ));
        }
        spans.push((comment_style, chars[pos..].iter().collect::<String>()));
        return spans;
    }

    while i < len {
        let c = chars[i];

        if c.is_whitespace() {
            let start = i;
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }
            spans.push((
                Style::default().fg(theme::TEXT_PRIMARY),
                chars[start..i].iter().collect(),
            ));
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            let start = i;
            i += 1;
            while i < len && chars[i] != quote {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            spans.push((string_style, chars[start..i].iter().collect()));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < len
                && (chars[i].is_ascii_digit()
                    || chars[i] == '-'
                    || chars[i] == ':'
                    || chars[i] == 'T'
                    || chars[i] == 'Z'
                    || chars[i] == '.'
                    || chars[i] == '+')
            {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let style = if token.contains('T') || token.contains('-') || token.contains(':') {
                date_style
            } else {
                number_style
            };
            spans.push((style, token));
            continue;
        }
        if chars[i..].starts_with(&['t', 'r', 'u', 'e']) {
            spans.push((bool_style, "true".to_string()));
            i += 4;
            continue;
        }
        if chars[i..].starts_with(&['f', 'a', 'l', 's', 'e']) {
            spans.push((bool_style, "false".to_string()));
            i += 5;
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '-' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-') {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            let style = if j < len && chars[j] == '=' {
                key_style
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            spans.push((style, token));
            continue;
        }
        spans.push((punct_style, c.to_string()));
        i += 1;
    }

    if spans.is_empty() {
        spans.push((Style::default().fg(theme::TEXT_PRIMARY), line.to_string()));
    }
    spans
}

fn toml_highlight_line_no_comment(line: &str) -> Vec<(Style, String)> {
    toml_highlight_line(line)
}

fn rust_highlight_line(line: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    let keyword_style = Style::default()
        .fg(theme::ACCENT_BLUE)
        .add_modifier(Modifier::BOLD);
    let string_style = Style::default().fg(theme::ACCENT_GREEN);
    let char_style = Style::default().fg(theme::ACCENT_GREEN);
    let comment_style = Style::default().fg(theme::TEXT_DIM);
    let number_style = Style::default().fg(theme::ACCENT_ORANGE);
    let attr_style = Style::default().fg(theme::ACCENT_MAGENTA);
    let macro_style = Style::default().fg(theme::ACCENT_YELLOW);
    let plain_style = Style::default().fg(theme::TEXT_PRIMARY);

    if let Some(pos) = line.find("//") {
        if pos > 0 {
            spans.extend(rust_highlight_line_no_comment(&line[..pos]));
        }
        spans.push((comment_style, line[pos..].to_string()));
        return spans;
    }

    while i < len {
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            spans.push((comment_style, line[i..].to_string()));
            return spans;
        }
        if bytes[i] == b'#' {
            let start = i;
            i += 1;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < len && bytes[i] == b'[' {
                let mut depth = 1;
                i += 1;
                while i < len && depth > 0 {
                    if bytes[i] == b'[' {
                        depth += 1;
                    }
                    if bytes[i] == b']' {
                        depth -= 1;
                    }
                    i += 1;
                }
                spans.push((attr_style, line[start..i].to_string()));
                continue;
            } else {
                spans.push((plain_style, line[start..i].to_string()));
                continue;
            }
        }
        if i + 1 < len && bytes[i] == b'r' {
            let mut hashes = 0usize;
            let mut j = i + 1;
            while j < len && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < len && bytes[j] == b'"' {
                let start = i;
                i = j + 1;
                let closing = format!("{}\"", "#".repeat(hashes));
                if let Some(pos) = line[i..].find(&closing) {
                    i += pos + closing.len();
                } else {
                    i = len;
                }
                spans.push((string_style, line[start..i].to_string()));
                continue;
            }
        }
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push((string_style, line[start..i].to_string()));
            continue;
        }
        if bytes[i] == b'\'' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'\\' && i + 1 < len {
                i += 2;
            } else if i < len {
                i += 1;
            }
            if i < len && bytes[i] == b'\'' {
                i += 1;
            }
            spans.push((char_style, line[start..i].to_string()));
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_' || bytes[i] == b'.') {
                i += 1;
            }
            spans.push((number_style, line[start..i].to_string()));
            continue;
        }
        if is_rust_ident_start(bytes[i]) {
            let start = i;
            while i < len && is_rust_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &line[start..i];
            let style = if RUST_KEYWORDS.contains(&word) {
                keyword_style
            } else if i < len && bytes[i] == b'!' {
                i += 1;
                macro_style
            } else {
                plain_style
            };
            spans.push((style, line[start..i].to_string()));
            continue;
        }
        spans.push((plain_style, line[i..i + 1].to_string()));
        i += 1;
    }

    if spans.is_empty() {
        spans.push((plain_style, line.to_string()));
    }
    spans
}

fn rust_highlight_line_no_comment(line: &str) -> Vec<(Style, String)> {
    rust_highlight_line(line)
}

fn is_rust_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_rust_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

const RUST_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "use", "struct", "impl", "if", "else", "match", "return", "async",
    "await", "trait", "enum", "type", "const", "static", "unsafe", "where", "for", "while", "loop",
    "break", "continue", "in", "ref", "move", "box", "dyn", "Self", "self", "super", "crate",
    "mod", "as",
];

fn python_highlight_line(line: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    let keyword_style = Style::default()
        .fg(theme::ACCENT_BLUE)
        .add_modifier(Modifier::BOLD);
    let string_style = Style::default().fg(theme::ACCENT_GREEN);
    let comment_style = Style::default().fg(theme::TEXT_DIM);
    let number_style = Style::default().fg(theme::ACCENT_ORANGE);
    let decorator_style = Style::default().fg(theme::ACCENT_MAGENTA);
    let plain_style = Style::default().fg(theme::TEXT_PRIMARY);

    if let Some(pos) = line.find('#') {
        if pos > 0 {
            spans.extend(python_highlight_line_no_comment(&line[..pos]));
        }
        spans.push((comment_style, line[pos..].to_string()));
        return spans;
    }
    if bytes.get(i) == Some(&b'@') {
        let start = i;
        i += 1;
        while i < len && is_py_ident_continue(bytes[i]) {
            i += 1;
        }
        spans.push((decorator_style, line[start..i].to_string()));
    }

    while i < len {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
            let triple = i + 2 < len && bytes[i + 1] == quote && bytes[i + 2] == quote;
            if triple {
                let close = &line[i + 3..];
                let end = close
                    .find(&line[i..i + 3])
                    .map(|p| p + i + 3)
                    .unwrap_or(len);
                spans.push((string_style, line[start..end].to_string()));
                i = end;
                continue;
            }
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push((string_style, line[start..i].to_string()));
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            spans.push((number_style, line[start..i].to_string()));
            continue;
        }
        if is_py_ident_start(bytes[i]) {
            let start = i;
            while i < len && is_py_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &line[start..i];
            let style = if PYTHON_KEYWORDS.contains(&word) {
                keyword_style
            } else {
                plain_style
            };
            spans.push((style, line[start..i].to_string()));
            continue;
        }
        spans.push((plain_style, line[i..i + 1].to_string()));
        i += 1;
    }

    if spans.is_empty() {
        spans.push((plain_style, line.to_string()));
    }
    spans
}

fn python_highlight_line_no_comment(line: &str) -> Vec<(Style, String)> {
    python_highlight_line(line)
}

fn is_py_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_py_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

const PYTHON_KEYWORDS: &[&str] = &[
    "def", "class", "if", "else", "elif", "for", "while", "return", "import", "from", "as", "with",
    "try", "except", "finally", "raise", "lambda", "pass", "break", "continue", "global",
    "nonlocal", "assert", "yield", "del", "in", "is", "not", "or", "and", "True", "False", "None",
];

/// Return true when the first line names a common shell interpreter.
pub fn is_shell_shebang(first_line: &str) -> bool {
    let Some(rest) = first_line.trim().strip_prefix("#!") else {
        return false;
    };
    let mut parts = rest.split_whitespace();
    let Some(command) = parts.next() else {
        return false;
    };
    let interpreter = command.rsplit('/').next().unwrap_or(command);
    if is_shell_interpreter(interpreter) {
        return true;
    }
    if interpreter != "env" {
        return false;
    }
    parts
        .filter(|part| !part.starts_with('-'))
        .any(|part| is_shell_interpreter(part.rsplit('/').next().unwrap_or(part)))
}

fn is_shell_interpreter(name: &str) -> bool {
    matches!(name, "sh" | "bash" | "dash" | "zsh" | "ksh")
}

fn shell_highlight_line(line: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    let keyword_style = Style::default()
        .fg(theme::ACCENT_BLUE)
        .add_modifier(Modifier::BOLD);
    let string_style = Style::default().fg(theme::ACCENT_GREEN);
    let comment_style = Style::default().fg(theme::TEXT_DIM);
    let number_style = Style::default().fg(theme::ACCENT_ORANGE);
    let variable_style = Style::default().fg(theme::ACCENT_MAGENTA);
    let operator_style = Style::default().fg(theme::TEXT_DIM);
    let plain_style = Style::default().fg(theme::TEXT_PRIMARY);

    while i < len {
        if bytes[i].is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            spans.push((plain_style, line[start..i].to_string()));
            continue;
        }
        if bytes[i] == b'#' {
            spans.push((comment_style, line[i..].to_string()));
            return spans;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < len {
                if quote == b'"' && bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            spans.push((string_style, line[start..i].to_string()));
            continue;
        }
        if bytes[i] == b'$' {
            let start = i;
            i += 1;
            if i < len && bytes[i] == b'{' {
                i += 1;
                while i < len && bytes[i] != b'}' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            } else if i < len && is_shell_special_param(bytes[i]) {
                i += 1;
            } else {
                while i < len && is_shell_ident_continue(bytes[i]) {
                    i += 1;
                }
            }
            let style = if i > start + 1 {
                variable_style
            } else {
                plain_style
            };
            spans.push((style, line[start..i].to_string()));
            continue;
        }
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'_') {
                i += 1;
            }
            spans.push((number_style, line[start..i].to_string()));
            continue;
        }
        if i + 1 < len
            && ((bytes[i] == b'[' && bytes[i + 1] == b'[')
                || (bytes[i] == b']' && bytes[i + 1] == b']'))
        {
            spans.push((keyword_style, line[i..i + 2].to_string()));
            i += 2;
            continue;
        }
        if is_shell_ident_start(bytes[i]) {
            let start = i;
            while i < len && is_shell_word_continue(bytes[i]) {
                i += 1;
            }
            let word = &line[start..i];
            let style = if SHELL_KEYWORDS.contains(&word) {
                keyword_style
            } else {
                plain_style
            };
            spans.push((style, line[start..i].to_string()));
            continue;
        }
        if is_shell_operator(bytes[i]) {
            spans.push((operator_style, line[i..i + 1].to_string()));
            i += 1;
            continue;
        }
        let ch = line[i..].chars().next().expect("valid char boundary");
        let end = i + ch.len_utf8();
        spans.push((plain_style, line[i..end].to_string()));
        i = end;
    }

    if spans.is_empty() {
        spans.push((plain_style, line.to_string()));
    }
    spans
}

fn is_shell_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_shell_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_shell_word_continue(b: u8) -> bool {
    is_shell_ident_continue(b) || b == b'-'
}

fn is_shell_special_param(b: u8) -> bool {
    b.is_ascii_digit() || matches!(b, b'@' | b'*' | b'#' | b'?' | b'!' | b'$' | b'-' | b'_')
}

fn is_shell_operator(b: u8) -> bool {
    matches!(
        b,
        b';' | b'&' | b'|' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'<' | b'>' | b'='
    )
}

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "coproc", "break", "continue", "return", "exit", "export",
    "local", "readonly", "declare", "typeset", "unset", "set", "shift", "getopts", "true", "false",
    "test",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    #[test]
    fn plain_highlighter_returns_unstyled() {
        let h = Highlighter::Plain;
        let spans = h.highlight_line("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].1, "hello world");
    }

    #[test]
    fn markdown_highlights_inline_formatting() {
        let spans = Highlighter::Markdown.highlight_line("**bold** *italic* `code` [x](y)");
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, "**bold** *italic* `code` [x](y)");
        assert!(spans.iter().any(
            |(style, s)| s.contains("**bold**") && style.add_modifier.contains(Modifier::BOLD)
        ));
        assert!(spans.iter().any(|(_, s)| s.contains("`code`")));
    }

    #[test]
    fn json_highlights_string_and_number() {
        let h = Highlighter::Json;
        let spans = h.highlight_line(r#"  \"key\": 42,"#);
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, r#"  \"key\": 42,"#);
        assert!(spans.len() > 1);
    }

    #[test]
    fn toml_highlights_comment() {
        let h = Highlighter::Toml;
        let spans = h.highlight_line("key = \"value\" # comment");
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, "key = \"value\" # comment");
        assert!(spans.len() > 1);
    }

    #[test]
    fn rust_highlights_keywords_and_comments() {
        let h = Highlighter::Rust;
        let spans = h.highlight_line("pub fn main() { // hi");
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, "pub fn main() { // hi");
        assert!(spans.len() > 1);
    }

    #[test]
    fn python_highlights_def_and_numbers() {
        let h = Highlighter::Python;
        let spans = h.highlight_line("def hello(x=3):");
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, "def hello(x=3):");
        assert!(spans.len() > 1);
    }

    #[test]
    fn shell_highlighter_detects_shell_extensions() {
        assert!(matches!(
            Highlighter::for_path(Some(std::path::Path::new("script.sh"))),
            Highlighter::Shell
        ));
        assert!(matches!(
            Highlighter::for_path(Some(std::path::Path::new("script.bash"))),
            Highlighter::Shell
        ));
    }

    #[test]
    fn shell_highlighter_detects_common_shebangs() {
        for shebang in [
            "#!/bin/sh",
            "#!/bin/bash",
            "#!/usr/bin/env sh",
            "#!/usr/bin/env bash",
            "#!/usr/bin/env -S bash -euo pipefail",
        ] {
            assert!(is_shell_shebang(shebang), "{shebang}");
            assert!(matches!(
                Highlighter::for_path_or_shebang(
                    Some(std::path::Path::new("configure")),
                    Some(shebang)
                ),
                Highlighter::Shell
            ));
        }
    }

    #[test]
    fn shell_highlights_keywords_variables_strings_and_comments() {
        let spans =
            Highlighter::Shell.highlight_line(r#"if [[ "$name" = root ]]; then echo $HOME # hi"#);
        let text: String = spans.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, r#"if [[ "$name" = root ]]; then echo $HOME # hi"#);
        assert!(
            spans
                .iter()
                .any(|(style, s)| s == "if" && style.add_modifier.contains(Modifier::BOLD))
        );
        assert!(spans.iter().any(|(_, s)| s == "$HOME"));
        assert!(spans.iter().any(|(_, s)| s == "# hi"));
    }
}
