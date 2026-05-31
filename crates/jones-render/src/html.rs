use jones_theme as theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

/// Render HTML content to styled terminal text.
/// Handles common block/inline elements, style nesting, HTML entities,
/// and skips script/style content.
pub fn render_html(input: &str) -> Text<'static> {
    let mut state = RenderState::new();

    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut text_buf = String::new();

    for ch in input.chars() {
        if ch == '<' {
            // Flush accumulated text
            if !text_buf.is_empty() {
                state.push_text(&text_buf);
                text_buf.clear();
            }
            in_tag = true;
            tag_buf.clear();
            continue;
        }

        if ch == '>' {
            in_tag = false;
            let tag = tag_buf.trim().to_lowercase();
            state.apply_tag(&tag);
            tag_buf.clear();
            continue;
        }

        if in_tag {
            tag_buf.push(ch);
        } else if !state.skip_content {
            text_buf.push(ch);
        }
    }

    // Flush remaining text
    if !text_buf.is_empty() {
        state.push_text(&text_buf);
    }
    state.flush_line();

    if state.lines.is_empty() {
        state.lines.push(Line::from(""));
    }

    Text::from(state.lines)
}

// ── Style tracking ───────────────────────────────────────────────────

struct StyleState {
    base: Style,
    bold: bool,
    italic: bool,
    link: bool,
    code: bool,
}

impl StyleState {
    fn new() -> Self {
        Self {
            base: Style::default().fg(theme::text_primary()),
            bold: false,
            italic: false,
            link: false,
            code: false,
        }
    }

    fn computed(&self) -> Style {
        let mut s = self.base;
        if self.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.link {
            s = s.fg(theme::link()).add_modifier(Modifier::UNDERLINED);
        }
        if self.code {
            s = s.fg(theme::code_fg()).bg(theme::code_bg());
        }
        s
    }
}

// ── List context ─────────────────────────────────────────────────────

enum ListKind {
    Unordered,
    Ordered(usize),
}

// ── Render state ─────────────────────────────────────────────────────

struct RenderState {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style: StyleState,
    base_stack: Vec<Style>,
    list_stack: Vec<ListKind>,
    in_pre: bool,
    skip_content: bool,
    last_was_space: bool,
}

impl RenderState {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            style: StyleState::new(),
            base_stack: Vec::new(),
            list_stack: Vec::new(),
            in_pre: false,
            skip_content: false,
            last_was_space: true, // suppress leading whitespace
        }
    }

    fn push_text(&mut self, text: &str) {
        let decoded = decode_entities(text);
        if self.in_pre {
            // Preserve whitespace in <pre> blocks
            for (i, line) in decoded.split('\n').enumerate() {
                if i > 0 {
                    self.flush_line();
                }
                if !line.is_empty() {
                    self.current_spans
                        .push(Span::styled(line.to_string(), self.style.computed()));
                    self.last_was_space = false;
                }
            }
        } else {
            // Collapse whitespace, preserving inter-element spaces
            let mut chunk = String::new();
            for ch in decoded.chars() {
                if ch.is_whitespace() {
                    if !self.last_was_space && (!self.current_spans.is_empty() || !chunk.is_empty())
                    {
                        chunk.push(' ');
                        self.last_was_space = true;
                    }
                } else {
                    chunk.push(ch);
                    self.last_was_space = false;
                }
            }
            if !chunk.is_empty() {
                self.current_spans
                    .push(Span::styled(chunk, self.style.computed()));
            }
        }
    }

    fn flush_line(&mut self) {
        self.lines
            .push(Line::from(std::mem::take(&mut self.current_spans)));
        self.last_was_space = true; // suppress leading whitespace on new line
    }

    fn flush_if_nonempty(&mut self) {
        if !self.current_spans.is_empty() {
            self.flush_line();
        }
    }

    fn blank_line(&mut self) {
        self.flush_if_nonempty();
        self.lines.push(Line::from(""));
    }

    fn push_base(&mut self, new_base: Style) {
        self.base_stack.push(self.style.base);
        self.style.base = new_base;
    }

    fn pop_base(&mut self) {
        if let Some(prev) = self.base_stack.pop() {
            self.style.base = prev;
        } else {
            self.style.base = Style::default();
        }
    }

    fn list_indent(&self) -> String {
        let depth = self.list_stack.len().saturating_sub(1);
        "  ".repeat(depth)
    }

    fn apply_tag(&mut self, tag: &str) {
        let tag_name = tag.split_whitespace().next().unwrap_or("");
        // Strip trailing / for self-closing tags like <br/> or <hr/>
        let tag_name = tag_name.trim_end_matches('/');

        match tag_name {
            // ── Block elements ───────────────────────────────────
            "p" | "div" => {
                self.flush_if_nonempty();
            }
            "/p" | "/div" => {
                self.blank_line();
            }
            "br" => {
                self.flush_line();
            }

            // ── Headings ─────────────────────────────────────────
            s if is_heading_open(s) => {
                self.blank_line();
                let level = s.as_bytes()[1] - b'0';
                let color = match level {
                    1 => theme::heading_h1(),
                    2 => theme::heading_h2(),
                    3 => theme::heading_h3(),
                    4 => theme::heading_h4(),
                    _ => theme::heading_default(),
                };
                self.push_base(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            s if is_heading_close(s) => {
                self.flush_line();
                self.lines.push(Line::from(""));
                self.pop_base();
            }

            // ── Inline styles ────────────────────────────────────
            "b" | "strong" => {
                self.style.bold = true;
            }
            "/b" | "/strong" => {
                self.style.bold = false;
            }
            "i" | "em" | "cite" => {
                self.style.italic = true;
            }
            "/i" | "/em" | "/cite" => {
                self.style.italic = false;
            }
            "a" => {
                self.style.link = true;
            }
            "/a" => {
                self.style.link = false;
            }
            "u" | "ins" => { /* underline handled via modifier if desired */ }
            "/u" | "/ins" => {}
            "s" | "del" | "strike" => { /* strikethrough not well supported in terminals */ }
            "/s" | "/del" | "/strike" => {}

            // ── Code ─────────────────────────────────────────────
            "code" => {
                self.style.code = true;
            }
            "/code" => {
                self.style.code = false;
            }
            "pre" => {
                self.flush_if_nonempty();
                self.in_pre = true;
                self.push_base(
                    Style::default()
                        .fg(theme::code_block_fg())
                        .bg(theme::code_block_bg()),
                );
            }
            "/pre" => {
                self.flush_line();
                self.in_pre = false;
                self.pop_base();
            }

            // ── Lists ────────────────────────────────────────────
            "ul" => {
                self.flush_if_nonempty();
                self.list_stack.push(ListKind::Unordered);
            }
            "/ul" => {
                self.flush_if_nonempty();
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.lines.push(Line::from(""));
                }
            }
            "ol" => {
                self.flush_if_nonempty();
                self.list_stack.push(ListKind::Ordered(0));
            }
            "/ol" => {
                self.flush_if_nonempty();
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.lines.push(Line::from(""));
                }
            }
            "li" => {
                self.flush_if_nonempty();
                let indent = self.list_indent();
                let bullet = match self.list_stack.last_mut() {
                    Some(ListKind::Unordered) => {
                        format!("{indent}  \u{2022} ")
                    }
                    Some(ListKind::Ordered(n)) => {
                        *n += 1;
                        format!("{indent}  {}. ", *n)
                    }
                    None => "  \u{2022} ".to_string(),
                };
                self.current_spans.push(Span::styled(
                    bullet,
                    Style::default().fg(theme::list_bullet()),
                ));
            }
            "/li" => {
                self.flush_line();
            }

            // ── Blockquote ───────────────────────────────────────
            "blockquote" => {
                self.blank_line();
                self.push_base(Style::default().fg(theme::blockquote()));
                self.current_spans.push(Span::styled(
                    "\u{2502} ".to_string(),
                    Style::default().fg(theme::blockquote()),
                ));
            }
            "/blockquote" => {
                self.flush_if_nonempty();
                self.pop_base();
                self.lines.push(Line::from(""));
            }

            // ── Horizontal rule ──────────────────────────────────
            "hr" => {
                self.flush_if_nonempty();
                self.lines.push(Line::from(Span::styled(
                    "\u{2500}".repeat(40),
                    Style::default().fg(theme::rule()),
                )));
            }

            // ── Images ───────────────────────────────────────────
            s if s.starts_with("img") => {
                // Extract alt text if present
                if let Some(alt) = extract_attr(tag, "alt")
                    && !alt.is_empty()
                {
                    self.current_spans.push(Span::styled(
                        format!("[{alt}]"),
                        Style::default().fg(theme::text_secondary()),
                    ));
                }
            }

            // ── Table basics ─────────────────────────────────────
            "table" => {
                self.blank_line();
            }
            "/table" => {
                self.lines.push(Line::from(""));
            }
            "tr" => {
                self.flush_if_nonempty();
            }
            "/tr" => {
                self.flush_line();
            }
            "td" | "th" => {
                if !self.current_spans.is_empty() {
                    self.current_spans.push(Span::raw("  |  ".to_string()));
                }
                if tag_name == "th" {
                    self.style.bold = true;
                }
            }
            "/td" => {}
            "/th" => {
                self.style.bold = false;
            }

            // ── Skip content ─────────────────────────────────────
            "script" | "style" | "noscript" => {
                self.skip_content = true;
            }
            "/script" | "/style" | "/noscript" => {
                self.skip_content = false;
            }

            // ── Misc ─────────────────────────────────────────────
            "figcaption" => {
                self.flush_if_nonempty();
                self.style.italic = true;
            }
            "/figcaption" => {
                self.flush_line();
                self.style.italic = false;
            }
            "figure" | "/figure" | "span" | "/span" | "sup" | "/sup" | "sub" | "/sub" | "small"
            | "/small" | "abbr" | "/abbr" | "mark" | "/mark" | "time" | "/time" | "aside"
            | "/aside" | "section" | "/section" | "article" | "/article" | "header" | "/header"
            | "footer" | "/footer" | "nav" | "/nav" | "main" | "/main" | "tbody" | "/tbody"
            | "thead" | "/thead" | "tfoot" | "/tfoot" | "colgroup" | "/colgroup" | "col"
            | "caption" | "/caption" | "details" | "/details" | "summary" | "/summary" | "dl"
            | "/dl" | "dd" | "/dd" | "dt" | "/dt" | "label" | "/label" | "form" | "/form"
            | "input" | "button" | "/button" | "select" | "/select" | "option" | "/option"
            | "textarea" | "/textarea" | "fieldset" | "/fieldset" | "legend" | "/legend"
            | "iframe" | "/iframe" | "embed" | "object" | "/object" | "source" | "video"
            | "/video" | "audio" | "/audio" | "picture" | "/picture" | "map" | "/map" | "area"
            | "!--" | "meta" | "link" => {
                // Known but unstyled — ignore silently
            }

            _ => {
                // Unknown tags — ignore
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn is_heading_open(s: &str) -> bool {
    s.len() == 2
        && s.starts_with('h')
        && s.as_bytes()[1].is_ascii_digit()
        && s.as_bytes()[1] >= b'1'
        && s.as_bytes()[1] <= b'6'
}

fn is_heading_close(s: &str) -> bool {
    s.len() == 3
        && s.starts_with("/h")
        && s.as_bytes()[2].is_ascii_digit()
        && s.as_bytes()[2] >= b'1'
        && s.as_bytes()[2] <= b'6'
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    for quote in ['"', '\''] {
        let pattern = format!("{attr_name}={quote}");
        if let Some(start) = lower.find(&pattern) {
            let value_start = start + pattern.len();
            // Guard against byte offset mismatch on non-ASCII tags
            if value_start <= tag.len()
                && let Some(end) = tag[value_start..].find(quote)
            {
                return Some(tag[value_start..value_start + end].to_string());
            }
        }
    }
    None
}

pub fn decode_entities(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '&' {
            result.push(ch);
            continue;
        }

        // Collect entity name up to ';'
        let mut entity = String::new();
        let mut found_semi = false;
        for ec in chars.by_ref() {
            if ec == ';' {
                found_semi = true;
                break;
            }
            entity.push(ec);
            if entity.len() > 10 {
                break; // Not a real entity
            }
        }

        if !found_semi {
            // Not a valid entity — emit as literal
            result.push('&');
            result.push_str(&entity);
            continue;
        }

        // Decode the entity
        if let Some(decoded) = decode_named_entity(&entity) {
            result.push_str(decoded);
        } else if let Some(stripped) = entity.strip_prefix('#') {
            if let Some(ch) = decode_numeric_entity(stripped) {
                result.push(ch);
            } else {
                result.push('&');
                result.push_str(&entity);
                result.push(';');
            }
        } else {
            // Unknown entity — emit literal
            result.push('&');
            result.push_str(&entity);
            result.push(';');
        }
    }

    result
}

fn decode_named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" | "#39" => "'",
        "nbsp" => "\u{00A0}",
        "mdash" => "\u{2014}",
        "ndash" => "\u{2013}",
        "hellip" => "\u{2026}",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201C}",
        "rdquo" => "\u{201D}",
        "copy" => "\u{00A9}",
        "reg" => "\u{00AE}",
        "trade" => "\u{2122}",
        "deg" => "\u{00B0}",
        "bull" | "bullet" => "\u{2022}",
        "middot" => "\u{00B7}",
        "laquo" => "\u{00AB}",
        "raquo" => "\u{00BB}",
        "times" => "\u{00D7}",
        "divide" => "\u{00F7}",
        "micro" => "\u{00B5}",
        "para" => "\u{00B6}",
        "sect" => "\u{00A7}",
        "euro" => "\u{20AC}",
        "pound" => "\u{00A3}",
        "yen" => "\u{00A5}",
        "cent" => "\u{00A2}",
        "frac12" => "\u{00BD}",
        "frac14" => "\u{00BC}",
        "frac34" => "\u{00BE}",
        "iexcl" => "\u{00A1}",
        "iquest" => "\u{00BF}",
        "shy" => "",
        "zwj" => "\u{200D}",
        "zwnj" => "\u{200C}",
        "ensp" => "\u{2002}",
        "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}",
        _ => return None,
    })
}

fn decode_numeric_entity(s: &str) -> Option<char> {
    let code = if let Some(hex) = s.strip_prefix('x').or_else(|| s.strip_prefix('X')) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        s.parse::<u32>().ok()?
    };
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_text(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(
            decode_entities("Tom &amp; Jerry &#33; &#x3F;"),
            "Tom & Jerry ! ?"
        );
    }

    #[test]
    fn renders_common_block_and_inline_html() {
        let rendered = render_html(
            "<h1>Title</h1><p>Hello <strong>bold</strong> <em>italic</em> <code>x</code></p>",
        );
        let all_text = collect_text(&rendered);

        assert!(all_text.contains("Title"));
        assert!(all_text.contains("Hello bold italic x"));
    }

    #[test]
    fn plain_html_text_uses_primary_text_color() {
        let rendered = render_html("<p>Hello</p>");
        let span = rendered.lines[0].spans.first().expect("plain text span");

        assert_eq!(span.style.fg, Some(theme::text_primary()));
    }

    #[test]
    fn renders_lists_blockquotes_and_preformatted_code() {
        let rendered = render_html(
            "<blockquote>quoted</blockquote><ul><li>one</li><li>two</li></ul><pre>fn main() {\n}</pre>",
        );
        let all_text = collect_text(&rendered);

        assert!(all_text.contains("│ quoted"));
        assert!(all_text.contains("• one"));
        assert!(all_text.contains("• two"));
        assert!(all_text.contains("fn main() {"));
    }
}
