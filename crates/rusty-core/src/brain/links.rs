//! Wikilinks, scanned one way for every part of the brain: the indexer, the renderer,
//! the migration and the move rewrite. `[[target#heading|alias]]` and `![[target]]`
//! (an embed) are the forms; fenced code and inline code are never links.

/// One wikilink as written in a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiRef {
    /// The target as written, without the heading or block part and without the alias.
    pub target: String,
    /// `#heading` or `^block` after the target, without the marker.
    pub fragment: Option<String>,
    /// The display text after `|`.
    pub alias: Option<String>,
    /// `![[...]]`.
    pub embed: bool,
    /// The line the link sits on, for backlink context.
    pub line: String,
    /// Zero-based line number.
    pub line_no: usize,
}

/// A line-aware walk over `text` that says whether each line is inside a fenced code
/// block. Returns `(line_no, line, in_fence)`.
fn lines_with_fences(text: &str) -> impl Iterator<Item = (usize, &str, bool)> {
    let mut in_fence = false;
    text.lines().enumerate().map(move |(n, line)| {
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let inside = in_fence;
        if is_fence {
            in_fence = !in_fence;
            return (n, line, true);
        }
        (n, line, inside)
    })
}

/// Every wikilink in `text`, in document order.
pub fn scan(text: &str) -> Vec<WikiRef> {
    let mut out = Vec::new();
    for (line_no, line, in_fence) in lines_with_fences(text) {
        if in_fence {
            continue;
        }
        let mut in_code = false;
        // Walk the line; toggle on backticks so `[[x]]` inside inline code is skipped.
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                in_code = !in_code;
                i += 1;
                continue;
            }
            if !in_code && bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                if let Some(end) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + end];
                    let embed = i > 0 && bytes[i - 1] == b'!';
                    if let Some(link) = parse_inner(inner, embed, line, line_no) {
                        out.push(link);
                    }
                    i += 2 + end + 2;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

/// Split the text between `[[` and `]]` into its parts.
fn parse_inner(inner: &str, embed: bool, line: &str, line_no: usize) -> Option<WikiRef> {
    let (spec, alias) = match inner.split_once('|') {
        Some((s, a)) => (s, Some(a.trim().to_string()).filter(|a| !a.is_empty())),
        None => (inner, None),
    };
    let (target, fragment) = match spec.find(['#', '^']) {
        Some(cut) => (
            &spec[..cut],
            Some(spec[cut + 1..].trim().to_string()).filter(|f| !f.is_empty()),
        ),
        None => (spec, None),
    };
    let target = target.trim();
    if target.is_empty() && fragment.is_none() {
        return None;
    }
    Some(WikiRef {
        target: target.to_string(),
        fragment,
        alias,
        embed,
        line: line.trim().to_string(),
        line_no,
    })
}

/// Distinct targets in `text`, in order of first appearance (the indexer's view).
pub fn targets(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    scan(text)
        .into_iter()
        .filter(|l| !l.target.is_empty())
        .filter(|l| seen.insert(l.target.clone()))
        .map(|l| l.target)
        .collect()
}

/// Every inline `#tag` in `text`, in order of first appearance, deduplicated without
/// case: a `#` at the start of a line or after whitespace, `(` or `,`, then letters,
/// digits, `_`, `-` and `/` with at least one letter; nothing inside code. A trailing
/// `/` is dropped. Frontmatter tags are not read here.
pub fn tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, line, in_fence) in lines_with_fences(text) {
        if in_fence {
            continue;
        }
        let mut in_code = false;
        let mut prev: Option<char> = None;
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '`' {
                in_code = !in_code;
                prev = Some(c);
                continue;
            }
            if in_code || c != '#' {
                prev = Some(c);
                continue;
            }
            let boundary = prev.is_none_or(|p| p.is_whitespace() || p == '(' || p == ',');
            let rest = &line[i + 1..];
            let len = rest
                .char_indices()
                .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/'))
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            let tag = rest[..len].trim_end_matches('/');
            if boundary && !tag.is_empty() && tag.chars().any(char::is_alphabetic) {
                if seen.insert(tag.to_lowercase()) {
                    out.push(tag.to_string());
                }
                for _ in 0..len {
                    chars.next();
                }
                prev = Some(' ');
                continue;
            }
            prev = Some(c);
        }
    }
    out
}

/// A link target normalised the way the vault resolves it: no leading `/`, no `.md`.
pub fn normalise_target(target: &str) -> String {
    let t = target.trim().trim_start_matches('/');
    let t = t.strip_prefix("./").unwrap_or(t);
    t.strip_suffix(".md").unwrap_or(t).to_string()
}

/// Rewrite every wikilink and markdown link whose target `map` renames. `map` sees the
/// target as written (normalised) and returns the new target, or `None` to leave it.
/// Fenced code and inline code are left alone. Returns the text and how many links
/// changed.
pub fn rewrite_targets(text: &str, map: &dyn Fn(&str) -> Option<String>) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut changed = 0;
    let ends_with_newline = text.ends_with('\n');
    let mut first = true;
    for (_, line, in_fence) in lines_with_fences(text) {
        if !first {
            out.push('\n');
        }
        first = false;
        if in_fence {
            out.push_str(line);
            continue;
        }
        let (rewritten, n) = rewrite_line(line, map);
        changed += n;
        out.push_str(&rewritten);
    }
    if ends_with_newline {
        out.push('\n');
    }
    (out, changed)
}

fn rewrite_line(line: &str, map: &dyn Fn(&str) -> Option<String>) -> (String, usize) {
    let mut out = String::with_capacity(line.len());
    let mut changed = 0;
    let bytes = line.as_bytes();
    let mut in_code = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'`' {
            in_code = !in_code;
            out.push('`');
            i += 1;
            continue;
        }
        if in_code {
            let ch = line[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // [[target...]]
        if c == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(end) = line[i + 2..].find("]]") {
                let inner = &line[i + 2..i + 2 + end];
                let (spec, alias) = match inner.split_once('|') {
                    Some((s, a)) => (s, Some(a)),
                    None => (inner, None),
                };
                let cut = spec.find(['#', '^']).unwrap_or(spec.len());
                let (target, tail) = spec.split_at(cut);
                match map(&normalise_target(target)) {
                    Some(new) if new != target.trim() => {
                        out.push_str("[[");
                        out.push_str(&new);
                        out.push_str(tail);
                        if let Some(a) = alias {
                            out.push('|');
                            out.push_str(a);
                        }
                        out.push_str("]]");
                        changed += 1;
                    }
                    _ => {
                        out.push_str("[[");
                        out.push_str(inner);
                        out.push_str("]]");
                    }
                }
                i += 2 + end + 2;
                continue;
            }
        }
        // ](target) or ](<target>)
        if c == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            if let Some(end) = line[i + 2..].find(')') {
                let inner = &line[i + 2..i + 2 + end];
                let angled = inner.starts_with('<') && inner.ends_with('>');
                let raw = if angled {
                    &inner[1..inner.len() - 1]
                } else {
                    inner
                };
                let (dest, title) = match raw.split_once(' ') {
                    Some((d, t)) if !angled => (d, Some(t)),
                    _ => (raw, None),
                };
                let is_local = !dest.contains("://") && !dest.starts_with('#');
                if is_local {
                    if let Some(new) = map(&normalise_target(dest)) {
                        let keep_md = dest.ends_with(".md");
                        out.push_str("](");
                        if angled {
                            out.push('<');
                        }
                        out.push_str(&new);
                        if keep_md {
                            out.push_str(".md");
                        }
                        if angled {
                            out.push('>');
                        }
                        if let Some(t) = title {
                            out.push(' ');
                            out.push_str(t);
                        }
                        out.push(')');
                        changed += 1;
                        i += 2 + end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = line[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, changed)
}

/// The map a page move needs: the old slug in any of its spellings (exact, with `.md`,
/// with a leading `/`, or the bare file name when it was unique) becomes the new slug.
pub fn move_map(
    from: &str,
    to: &str,
    basename_was_unique: bool,
) -> impl Fn(&str) -> Option<String> {
    let from = from.to_string();
    let to = to.to_string();
    let basename = from.rsplit('/').next().unwrap_or(&from).to_string();
    move |target: &str| {
        if target.eq_ignore_ascii_case(&from) {
            return Some(to.clone());
        }
        if basename_was_unique && !target.contains('/') && target.eq_ignore_ascii_case(&basename) {
            return Some(to.clone());
        }
        None
    }
}

/// The map a folder move needs: every target under `from/` moves under `to/`.
pub fn folder_move_map(from: &str, to: &str) -> impl Fn(&str) -> Option<String> {
    let prefix = format!("{}/", from.trim_end_matches('/'));
    let to = to.trim_end_matches('/').to_string();
    move |target: &str| {
        let lower = target.to_ascii_lowercase();
        if lower.starts_with(&prefix.to_ascii_lowercase()) {
            return Some(format!("{to}/{}", &target[prefix.len()..]));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_reads_every_form_and_skips_code() {
        let text = "See [[projects/orbit#Goals|the goals]] and ![[img.png]].\n\
                    `[[not/a/link]]` and\n```\n[[nor/this]]\n```\n[[people/sarah-chen]]";
        let links = scan(text);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "projects/orbit");
        assert_eq!(links[0].fragment.as_deref(), Some("Goals"));
        assert_eq!(links[0].alias.as_deref(), Some("the goals"));
        assert!(!links[0].embed);
        assert_eq!(links[0].line_no, 0);
        assert!(links[1].embed);
        assert_eq!(links[1].target, "img.png");
        assert_eq!(links[2].target, "people/sarah-chen");
        assert_eq!(links[2].line_no, 5);
        assert_eq!(
            targets(text),
            vec!["projects/orbit", "img.png", "people/sarah-chen"]
        );
    }

    #[test]
    fn heading_only_links_keep_the_fragment() {
        let links = scan("[[#Section]] and [[a/b^blk]]");
        assert_eq!(links[0].target, "");
        assert_eq!(links[0].fragment.as_deref(), Some("Section"));
        assert_eq!(links[1].fragment.as_deref(), Some("blk"));
        assert_eq!(targets("[[#Section]]"), Vec::<String>::new());
    }

    #[test]
    fn move_rewrites_every_spelling_and_nothing_else() {
        let text = "[[a/b]] [[a/b|x]] [[a/b#h]] ![[a/b]] [[b]] [[A/B]] [[/a/b.md]]\n\
                    [text](a/b.md) [t](<a/b.md>) [t](a/b.md \"title\")\n\
                    [[a/bc]] `[[a/b]]`\n```\n[[a/b]]\n```\n";
        let map = move_map("a/b", "c/d", true);
        let (out, n) = rewrite_targets(text, &map);
        assert_eq!(n, 10);
        assert_eq!(
            out,
            "[[c/d]] [[c/d|x]] [[c/d#h]] ![[c/d]] [[c/d]] [[c/d]] [[c/d]]\n\
             [text](c/d.md) [t](<c/d.md>) [t](c/d.md \"title\")\n\
             [[a/bc]] `[[a/b]]`\n```\n[[a/b]]\n```\n"
        );
        let map = move_map("a/b", "c/d", false);
        let (out, n) = rewrite_targets("[[b]] [[a/b]]", &map);
        assert_eq!((out.as_str(), n), ("[[b]] [[c/d]]", 1));
    }

    #[test]
    fn folder_move_rewrites_by_prefix() {
        let map = folder_move_map("old", "new/deep");
        let (out, n) = rewrite_targets("[[old/a]] [[older/a]] [[Old/b|B]]", &map);
        assert_eq!(out, "[[new/deep/a]] [[older/a]] [[new/deep/b|B]]");
        assert_eq!(n, 2);
    }

    #[test]
    fn inline_tags_follow_obsidian_rules() {
        let text = "Plan #design/next and #Design (again) #123 a#b `#code`\n```\n#fenced\n```\nhttp://x.y/#frag, #ok/";
        assert_eq!(tags(text), vec!["design/next", "Design", "ok"]);
        assert!(tags("no tags # here").is_empty());
    }

    #[test]
    fn normalise_strips_what_obsidian_ignores() {
        assert_eq!(normalise_target("/a/b.md"), "a/b");
        assert_eq!(normalise_target("./a/b"), "a/b");
        assert_eq!(normalise_target(" a/b "), "a/b");
    }
}
