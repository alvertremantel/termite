use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub label: String,
    pub line: usize,
    pub depth: usize,
    pub kind: OutlineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineKind {
    Heading,
    Symbol,
    Section,
}

pub fn extract_outline(path: Option<&Path>, text: &str) -> Vec<OutlineEntry> {
    let ext = path
        .and_then(|p| p.extension())
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut out = match ext.as_str() {
        "md" | "markdown" => markdown(text),
        "rs" => rust(text),
        "py" => python(text),
        "toml" => toml(text),
        "json" => json(text),
        _ => fallback(text),
    };
    if out.is_empty() {
        out = fallback(text);
    }
    out
}

pub fn breadcrumb(entries: &[OutlineEntry], line: usize) -> Option<String> {
    let current = entries.iter().take_while(|e| e.line <= line).last()?;
    let mut stack: Vec<&OutlineEntry> = Vec::new();
    for e in entries.iter().filter(|e| e.line <= current.line) {
        while stack.last().is_some_and(|p| p.depth >= e.depth) {
            stack.pop();
        }
        stack.push(e);
    }
    Some(
        stack
            .into_iter()
            .map(|e| e.label.as_str())
            .collect::<Vec<_>>()
            .join(" › "),
    )
}

fn markdown(text: &str) -> Vec<OutlineEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start();
            let n = t.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&n) && t.as_bytes().get(n).is_some_and(|b| b.is_ascii_whitespace())
            {
                let label = t[n..].trim().trim_matches('#').trim().to_string();
                (!label.is_empty()).then_some(OutlineEntry {
                    label,
                    line: i,
                    depth: n,
                    kind: OutlineKind::Heading,
                })
            } else {
                None
            }
        })
        .collect()
}

fn rust(text: &str) -> Vec<OutlineEntry> {
    let keys = ["fn ", "struct ", "enum ", "impl ", "trait ", "mod "];
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start().trim_start_matches("pub ").trim_start();
            let key = keys.iter().find(|k| t.starts_with(**k))?;
            let rest = &t[key.len()..];
            let name = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '<'))
                .next()
                .unwrap_or("")
                .trim_end_matches('<');
            (!name.is_empty()).then_some(OutlineEntry {
                label: format!(
                    "{} {}",
                    key.trim(),
                    if *key == "impl " {
                        rest.split('{').next().unwrap_or(rest).trim()
                    } else {
                        name
                    }
                ),
                line: i,
                depth: brace_depth_before(text, i) + 1,
                kind: OutlineKind::Symbol,
            })
        })
        .collect()
}

fn python(text: &str) -> Vec<OutlineEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start();
            let kw = if t.starts_with("def ") {
                "def "
            } else if t.starts_with("async def ") {
                "async def "
            } else if t.starts_with("class ") {
                "class "
            } else {
                return None;
            };
            let name = t[kw.len()..]
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            (!name.is_empty()).then_some(OutlineEntry {
                label: format!("{} {}", kw.trim_end(), name),
                line: i,
                depth: l.len().saturating_sub(t.len()) / 4 + 1,
                kind: OutlineKind::Symbol,
            })
        })
        .collect()
}

fn toml(text: &str) -> Vec<OutlineEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim();
            if t.starts_with('[') && t.ends_with(']') {
                Some(OutlineEntry {
                    label: t.trim_matches(&['[', ']'][..]).to_string(),
                    line: i,
                    depth: 1,
                    kind: OutlineKind::Section,
                })
            } else {
                None
            }
        })
        .collect()
}

fn json(text: &str) -> Vec<OutlineEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start();
            if t.starts_with('"') && t.contains("\":") {
                let label = t[1..].split('"').next()?.to_string();
                Some(OutlineEntry {
                    label,
                    line: i,
                    depth: (l.len() - t.len()) / 2 + 1,
                    kind: OutlineKind::Section,
                })
            } else {
                None
            }
        })
        .collect()
}

fn fallback(text: &str) -> Vec<OutlineEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim();
            (!t.is_empty() && t.len() <= 80 && (i == 0 || t.ends_with(':'))).then_some(
                OutlineEntry {
                    label: t.to_string(),
                    line: i,
                    depth: 1,
                    kind: OutlineKind::Section,
                },
            )
        })
        .take(50)
        .collect()
}

fn brace_depth_before(text: &str, line: usize) -> usize {
    text.lines()
        .take(line)
        .map(|l| {
            l.matches('{')
                .count()
                .saturating_sub(l.matches('}').count())
        })
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markdown() {
        let o = extract_outline(Some(Path::new("a.md")), "# A\ntext\n## B\n");
        assert_eq!(o[1].label, "B");
        assert_eq!(breadcrumb(&o, 2).unwrap(), "A › B");
    }

    #[test]
    fn extracts_rust() {
        let o = extract_outline(
            Some(Path::new("lib.rs")),
            "pub struct App {}\nimpl App {\n fn run(&self) {}\n}\n",
        );
        assert!(o.iter().any(|e| e.label.contains("struct App")));
        assert!(o.iter().any(|e| e.label.contains("fn run")));
    }

    #[test]
    fn extracts_python() {
        let o = extract_outline(
            Some(Path::new("x.py")),
            "class A:\n    def b(self):\n        pass\n",
        );
        assert_eq!(o[0].label, "class A");
        assert_eq!(o[1].depth, 2);
    }

    #[test]
    fn extracts_toml_sections() {
        let o = extract_outline(Some(Path::new("Cargo.toml")), "[package]\nname = \"x\"\n");
        assert_eq!(o[0].label, "package");
        assert_eq!(o[0].kind, OutlineKind::Section);
    }

    #[test]
    fn extracts_json_keys() {
        let o = extract_outline(
            Some(Path::new("a.json")),
            "{\n  \"name\": \"termite\",\n  \"deps\": {\n    \"ropey\": true\n  }\n}\n",
        );
        assert_eq!(o[0].label, "name");
        assert_eq!(o[1].label, "deps");
    }

    #[test]
    fn fallback_extracts_first_line_and_colon_sections() {
        let o = extract_outline(None, "Title\n\nBody:\nMore text\n");
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].label, "Title");
        assert_eq!(o[1].label, "Body:");
    }
}
