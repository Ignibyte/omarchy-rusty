//! Obsidian-flavoured markdown rendered to HTML the way Obsidian's reading view shows it:
//! wikilinks with aliases and headings, page and image embeds, callouts, task lists,
//! tables, footnotes, highlights, tags, hidden comments, fenced code. `brain_render`
//! returns it to any client that wants HTML; a client that draws its own widgets asks for
//! the typed blocks in [`super::blocks`] instead.
//!
//! The HTML is a conservative subset with colours and fonts inlined from a [`Style`] and
//! no stylesheet, so it also suits rich-text engines such as Qt's. Links carry the
//! `rusty:` scheme (`rusty:page/<slug>`, `rusty:new/<name>`, `rusty:task/<n>`,
//! `rusty:tag/<tag>`) for the client to route.

use std::collections::HashMap;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::frontmatter::split_raw;
use super::links::normalise_target;

/// The colours and fonts the HTML is written with. Every field has a default, so a
/// partial JSON object (what a client sends) fills the rest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Body text.
    pub text: String,
    /// Secondary text: done tasks, quote text, footnote numbers.
    pub muted: String,
    /// Links to pages that exist.
    pub link: String,
    /// Links to pages that do not exist yet.
    pub unresolved: String,
    /// Accent for task boxes and quote bars.
    pub accent: String,
    /// Code text.
    pub code: String,
    /// Code and callout background.
    pub code_bg: String,
    /// Monospace family for code.
    pub mono: String,
    /// `==highlight==` background.
    pub mark_bg: String,
    /// Borders and rules.
    pub line: String,
    /// `#tag` colour.
    pub tag: String,
    /// Semantic colours, used by callouts.
    pub red: String,
    /// See [`Style::red`].
    pub green: String,
    /// See [`Style::red`].
    pub yellow: String,
    /// See [`Style::red`].
    pub blue: String,
    /// See [`Style::red`].
    pub magenta: String,
    /// See [`Style::red`].
    pub cyan: String,
    /// Heading colours, h1 to h6.
    pub headings: Vec<String>,
    /// Base font size in pixels; headings and code scale from it.
    pub size: f32,
    /// Titles: the first heading and the strongest text.
    pub bright: String,
    /// Section titles.
    pub gold: String,
    /// What is alive: done task boxes.
    pub alive: String,
    /// The accent where a surface takes it.
    pub accent_soft: String,
    /// A raised surface: code block headers.
    pub panel3: String,
    /// Lines that should be seen: open task boxes.
    pub line_bright: String,
    /// The mock's marks: `#` before headings in the accent, a rule under a section
    /// title, uppercase callout labels, task boxes in the line and alive colours.
    pub marks: bool,
    /// A header strip naming the language above a fenced code block.
    pub code_head: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            text: "#a9b1d6".into(),
            muted: "#787c99".into(),
            link: "#7aa2f7".into(),
            unresolved: "#565f89".into(),
            accent: "#7aa2f7".into(),
            code: "#449dab".into(),
            code_bg: "#1f2335".into(),
            mono: "JetBrainsMono Nerd Font".into(),
            mark_bg: "#4d4322".into(),
            line: "#444b6a".into(),
            tag: "#449dab".into(),
            red: "#f7768e".into(),
            green: "#9ece6a".into(),
            yellow: "#e0af68".into(),
            blue: "#7aa2f7".into(),
            magenta: "#ad8ee6".into(),
            cyan: "#7dcfff".into(),
            headings: vec![
                "#f7768e".into(),
                "#9ece6a".into(),
                "#e0af68".into(),
                "#7aa2f7".into(),
                "#ad8ee6".into(),
                "#ad8ee6".into(),
            ],
            size: 16.0,
            bright: "#c0caf5".into(),
            gold: "#e0af68".into(),
            alive: "#7dcfff".into(),
            accent_soft: "#3d59a1".into(),
            panel3: "#24283b".into(),
            line_bright: "#565f89".into(),
            marks: false,
            code_head: false,
        }
    }
}

impl Style {
    fn heading_colour(&self, level: usize) -> &str {
        self.headings
            .get(level.saturating_sub(1))
            .map(String::as_str)
            .unwrap_or(&self.text)
    }

    /// Obsidian's heading scale: 1.802, 1.602, 1.424, 1.266, 1.125, 1.0.
    fn heading_size(&self, level: usize) -> f32 {
        let factor = match level {
            1 => 1.802,
            2 => 1.602,
            3 => 1.424,
            4 => 1.266,
            5 => 1.125,
            _ => 1.0,
        };
        self.size * factor
    }
}

/// What the renderer needs from the vault.
pub trait Resolver {
    /// The slug a link target names, when a page exists for it.
    fn resolve(&self, target: &str) -> Option<String>;
    /// A page's title and raw file text, for embeds.
    fn page(&self, slug: &str) -> Option<(String, String)>;
    /// A URL for a file the target names (an image, say).
    fn file_url(&self, target: &str) -> Option<String>;
}

/// A resolver with no vault behind it: nothing resolves.
pub struct NoVault;

impl Resolver for NoVault {
    fn resolve(&self, _: &str) -> Option<String> {
        None
    }
    fn page(&self, _: &str) -> Option<(String, String)> {
        None
    }
    fn file_url(&self, _: &str) -> Option<String> {
        None
    }
}

/// A heading of the page, for the outline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    /// 1 to 6.
    pub level: u8,
    /// The heading text as written.
    pub text: String,
    /// Zero-based source line.
    pub line: usize,
}

/// A wikilink or embed the page makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkOut {
    /// The target as written.
    pub target: String,
    /// The slug it resolved to, when a page exists.
    pub slug: Option<String>,
    /// The display text, when the link had one.
    pub alias: Option<String>,
    /// Whether it was an embed.
    pub embed: bool,
}

/// The rendered page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rendered {
    /// Rich-text HTML.
    pub html: String,
    /// Headings in order.
    pub outline: Vec<Heading>,
    /// Wikilinks and embeds in order of appearance.
    pub links: Vec<LinkOut>,
    /// Distinct targets that resolved to no page.
    pub unresolved: Vec<String>,
    /// Task items (`- [ ]`, `- [x]`) in order, outside fenced code; `rusty:task/<n>` numbers
    /// them the same way, so a client can toggle the n-th item in the source.
    pub tasks: usize,
    /// Words in the body.
    pub words: usize,
    /// Characters in the body, spaces included.
    pub characters: usize,
}

/// The body of a page: the raw file without its frontmatter.
pub fn body_of(raw: &str) -> &str {
    match split_raw(raw) {
        Ok((_, body)) => body,
        Err(_) => raw,
    }
}

/// Render a page body. `self_slug` stops a page embedding itself.
pub fn render(
    body: &str,
    style: &Style,
    resolver: &dyn Resolver,
    self_slug: Option<&str>,
) -> Rendered {
    let body = mark_callouts(&strip_comments(body));
    let mut writer = Writer::new(style, resolver, 0, self_slug);
    writer.write_document(&body);
    let links = std::mem::take(&mut writer.links);
    let tasks = writer.tasks;
    let mut unresolved = Vec::new();
    for link in &links {
        if link.slug.is_none()
            && !link.target.is_empty()
            && !unresolved.contains(&link.target)
            && !(link.embed && looks_like_file(&link.target))
        {
            unresolved.push(link.target.clone());
        }
    }
    Rendered {
        html: writer.finish(),
        outline: outline(&body),
        links,
        unresolved,
        tasks,
        words: body.split_whitespace().count(),
        characters: body.chars().count(),
    }
}

/// The headings of a body, fenced code skipped.
pub fn outline(body: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (line_no, line) in body.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !trimmed.starts_with('#') {
            continue;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if level > 6 {
            continue;
        }
        let rest = &trimmed[level..];
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            continue;
        }
        let text = rest.trim().trim_end_matches('#').trim();
        out.push(Heading {
            level: level as u8,
            text: text.to_string(),
            line: line_no,
        });
    }
    out
}

/// Remove `%% comments %%`, single-line and spanning lines, outside fenced code.
pub fn strip_comments(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    let mut in_comment = false;
    let mut first = true;
    for line in body.lines() {
        if !first {
            out.push('\n');
        }
        first = false;
        let trimmed = line.trim_start();
        if !in_comment && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        loop {
            if in_comment {
                match rest.find("%%") {
                    Some(end) => {
                        in_comment = false;
                        rest = &rest[end + 2..];
                    }
                    None => break,
                }
            } else {
                match rest.find("%%") {
                    Some(start) => {
                        out.push_str(&rest[..start]);
                        in_comment = true;
                        rest = &rest[start + 2..];
                    }
                    None => {
                        out.push_str(rest);
                        break;
                    }
                }
            }
        }
    }
    if body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Written before every top-level heading in the HTML, so a client can split the page at
/// its headings.
pub const HEADING_MARK: &str = "<!--h-->";

/// The markers a callout head is rewritten with before parsing, so `[!kind]` reaches
/// the writer as one text run instead of the pieces the link parser makes of brackets.
const CALLOUT_OPEN: char = '\u{E000}';
const CALLOUT_CLOSE: char = '\u{E001}';

/// Rewrite `> [!kind]` heads with private-use markers; fenced code is left alone.
fn mark_callouts(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    let mut first = true;
    for line in body.lines() {
        if !first {
            out.push('\n');
        }
        first = false;
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence || !trimmed.starts_with('>') {
            out.push_str(line);
            continue;
        }
        let after_quote = trimmed[1..].trim_start();
        if let Some(rest) = after_quote.strip_prefix("[!") {
            if let Some(close) = rest.find(']') {
                let kind = &rest[..close];
                if !kind.is_empty()
                    && kind
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    let head_len = line.len() - after_quote.len();
                    out.push_str(&line[..head_len]);
                    out.push(CALLOUT_OPEN);
                    out.push_str(kind);
                    out.push(CALLOUT_CLOSE);
                    out.push_str(&rest[close + 1..]);
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    if body.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub(crate) fn looks_like_file(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".pdf", ".mp3", ".mp4", ".wav",
        ".webm", ".ogg", ".canvas", ".base", ".json", ".csv", ".txt",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

pub(crate) fn is_image(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// HTML-escape text.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Callout kinds as Obsidian names them, with a colour role and a glyph.
fn callout_look<'a>(kind: &str, style: &'a Style) -> (&'a str, &'static str) {
    match kind.to_ascii_lowercase().as_str() {
        "abstract" | "summary" | "tldr" => (&style.cyan, "≡"),
        "info" => (&style.blue, "ℹ"),
        "todo" => (&style.blue, "☐"),
        "tip" | "hint" | "important" => (&style.cyan, "✦"),
        "success" | "check" | "done" => (&style.green, "✓"),
        "question" | "help" | "faq" => (&style.yellow, "?"),
        "warning" | "caution" | "attention" => (&style.yellow, "⚠"),
        "failure" | "fail" | "missing" => (&style.red, "✕"),
        "danger" | "error" => (&style.red, "⚡"),
        "bug" => (&style.red, "✱"),
        "example" => (&style.magenta, "☰"),
        "quote" | "cite" => (&style.muted, "❝"),
        _ => (&style.accent, "✎"),
    }
}

/// `[!kind]+ Title` at the start of a blockquote's first line, as marked by
/// [`mark_callouts`].
fn parse_callout(text: &str) -> Option<(String, String, &str)> {
    let rest = text.strip_prefix(CALLOUT_OPEN)?;
    let close = rest.find(CALLOUT_CLOSE)?;
    let kind = rest[..close].trim();
    if kind.is_empty() {
        return None;
    }
    let mut after = &rest[close + CALLOUT_CLOSE.len_utf8()..];
    if let Some(a) = after.strip_prefix(['+', '-']) {
        after = a;
    }
    let (title, remainder) = match after.split_once('\n') {
        Some((t, r)) => (t, r),
        None => (after, ""),
    };
    let title = title.trim();
    let title = if title.is_empty() {
        let mut chars = kind.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    } else {
        title.to_string()
    };
    Some((kind.to_string(), title, remainder))
}

struct QuoteState {
    /// The opening HTML has been written.
    opened: bool,
}

struct Writer<'a> {
    style: &'a Style,
    resolver: &'a dyn Resolver,
    depth: usize,
    self_slug: Option<String>,
    out: String,
    footnotes: String,
    footnote_order: Vec<String>,
    in_footnote: bool,
    tasks: usize,
    links: Vec<LinkOut>,
    quotes: Vec<QuoteState>,
    /// A paragraph start inside a fresh blockquote is held until its first text says
    /// whether the quote is a callout.
    hold_paragraph: bool,
    /// Per open list item: whether a `<s>` for a done task is open.
    items: Vec<bool>,
    /// Alt text of the image being read, with its target and link type.
    image: Option<(String, LinkType, String)>,
    table_aligns: Vec<Alignment>,
    cell: usize,
    in_head: bool,
    skip: usize,
    /// The text of the fenced or indented code block being read.
    code: Option<String>,
    /// The fenced block's info string (its language), for the header strip.
    code_info: String,
    /// A paragraph ended inside a list item; the next one in the same item gets a break.
    item_break: bool,
    /// The number and label of a footnote definition whose first paragraph has not
    /// started yet.
    footnote_head: Option<(usize, String)>,
    /// The line break after a callout head is part of the head, not the body.
    skip_break: bool,
}

impl<'a> Writer<'a> {
    fn new(
        style: &'a Style,
        resolver: &'a dyn Resolver,
        depth: usize,
        self_slug: Option<&str>,
    ) -> Self {
        Self {
            style,
            resolver,
            depth,
            self_slug: self_slug.map(str::to_string),
            out: String::new(),
            footnotes: String::new(),
            footnote_order: Vec::new(),
            in_footnote: false,
            tasks: 0,
            links: Vec::new(),
            quotes: Vec::new(),
            hold_paragraph: false,
            items: Vec::new(),
            image: None,
            table_aligns: Vec::new(),
            cell: 0,
            in_head: false,
            skip: 0,
            code: None,
            code_info: String::new(),
            item_break: false,
            footnote_head: None,
            skip_break: false,
        }
    }

    fn push(&mut self, s: &str) {
        if self.in_footnote {
            self.footnotes.push_str(s);
        } else {
            self.out.push_str(s);
        }
    }

    fn finish(mut self) -> String {
        if !self.footnotes.is_empty() {
            self.out.push_str("<hr>");
            let notes = std::mem::take(&mut self.footnotes);
            self.out.push_str(&notes);
        }
        self.out
    }

    fn write_document(&mut self, body: &str) {
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_WIKILINKS
            | Options::ENABLE_MATH;
        let parser = Parser::new_ext(body, options);
        for event in parser {
            self.event(event);
        }
    }

    /// Write the opening of a plain quote (a bar in the accent colour).
    fn open_quote(&mut self) {
        let bar = self.style.accent.clone();
        self.push(&format!(
            "<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" style=\"margin-top:10px;margin-bottom:10px\"><tr><td width=\"3\" bgcolor=\"{bar}\"></td><td width=\"14\"></td><td>"
        ));
    }

    fn open_callout(&mut self, kind: &str, title: &str) {
        let (colour, glyph) = callout_look(kind, self.style);
        let colour = colour.to_string();
        let bg = self.style.code_bg.clone();
        let label = if self.style.marks {
            format!(
                "<span style=\"font-size:{}px\">{}</span>",
                self.style.size * 0.625,
                esc(&title.to_uppercase())
            )
        } else {
            esc(title)
        };
        self.push(&format!(
            "<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\" bgcolor=\"{bg}\" style=\"margin-top:10px;margin-bottom:10px\"><tr><td width=\"3\" bgcolor=\"{colour}\"></td><td width=\"14\"></td><td><p style=\"margin-bottom:2px\"><b><span style=\"color:{colour}\">{glyph}&nbsp; {label}</span></b></p>"
        ));
    }

    fn close_quote(&mut self) {
        self.push("</td><td width=\"14\"></td></tr></table>");
    }

    /// Flush a held paragraph start as a plain quote.
    fn flush_hold(&mut self) {
        if self.hold_paragraph {
            self.hold_paragraph = false;
            if let Some(q) = self.quotes.last_mut() {
                if !q.opened {
                    q.opened = true;
                    self.open_quote();
                }
            }
            self.push("<p>");
        }
    }

    fn event(&mut self, event: Event<'_>) {
        if self.skip > 0 {
            match event {
                Event::Start(_) => self.skip += 1,
                Event::End(_) => self.skip -= 1,
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.flush_hold();
                self.push(&format!(
                    "<span style=\"font-family:'{}';font-size:{}px;color:{};background-color:{}\">{}</span>",
                    esc(&self.style.mono),
                    self.style.size * 0.875,
                    self.style.code,
                    self.style.code_bg,
                    esc(&code)
                ));
            }
            Event::InlineMath(m) => {
                self.flush_hold();
                self.push(&format!(
                    "<span style=\"font-family:'{}';color:{}\">${}$</span>",
                    esc(&self.style.mono),
                    self.style.code,
                    esc(&m)
                ));
            }
            Event::DisplayMath(m) => {
                self.code_block(&m);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                self.flush_hold();
                self.push(&h);
            }
            Event::FootnoteReference(label) => {
                self.flush_hold();
                let n = self.footnote_number(&label);
                self.push(&format!(
                    "<sup><a href=\"#fn-{}\" style=\"color:{};text-decoration:none\">[{n}]</a></sup>",
                    esc(&label),
                    self.style.link
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.skip_break {
                    self.skip_break = false;
                    return;
                }
                self.flush_hold();
                self.push("<br>");
            }
            Event::Rule => {
                self.push(&format!("<hr style=\"color:{}\">", self.style.line));
            }
            Event::TaskListMarker(done) => {
                self.flush_hold();
                let n = self.tasks;
                self.tasks += 1;
                let glyph = if done { "☑" } else { "☐" };
                let colour = if !self.style.marks {
                    &self.style.accent
                } else if done {
                    &self.style.alive
                } else {
                    &self.style.line_bright
                };
                self.push(&format!(
                    "<a href=\"rusty:task/{n}\" style=\"text-decoration:none;color:{}\">{glyph}</a> ",
                    colour
                ));
                if done {
                    self.push(&format!("<s style=\"color:{}\">", self.style.muted));
                    if let Some(item) = self.items.last_mut() {
                        *item = true;
                    }
                }
            }
        }
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        match self.footnote_order.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.footnote_order.push(label.to_string());
                self.footnote_order.len()
            }
        }
    }

    fn code_block(&mut self, code: &str) {
        let head = if self.style.code_head && !self.code_info.is_empty() {
            format!(
                "<tr><td bgcolor=\"{}\"><span style=\"font-family:'{}';font-size:{}px;color:{}\">{}</span></td></tr>",
                self.style.panel3,
                esc(&self.style.mono),
                self.style.size * 0.625,
                self.style.muted,
                esc(&self.code_info.to_uppercase())
            )
        } else {
            String::new()
        };
        self.push(&format!(
            "<table width=\"100%\" cellspacing=\"0\" cellpadding=\"10\">{head}<tr><td bgcolor=\"{}\"><pre style=\"font-family:'{}';font-size:{}px;color:{}\">{}</pre></td></tr></table>",
            self.style.code_bg,
            esc(&self.style.mono),
            self.style.size * 0.875,
            self.style.code,
            esc(code.trim_end_matches('\n'))
        ));
    }

    /// Close a done task's strike-through before nested content or the item's end.
    fn close_task_strike(&mut self) {
        if let Some(item) = self.items.last_mut() {
            if *item {
                *item = false;
                self.push("</s>");
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if let Some((n, label)) = self.footnote_head.take() {
                    self.push(&format!(
                        "<p><a name=\"fn-{}\"></a><span style=\"color:{}\">{n}.</span> ",
                        esc(&label),
                        self.style.muted
                    ));
                } else if self.quotes.last().is_some_and(|q| !q.opened) {
                    self.hold_paragraph = true;
                } else if self.items.is_empty() {
                    self.push("<p>");
                } else if self.item_break {
                    self.item_break = false;
                    self.push("<br>");
                }
            }
            Tag::Heading { level, .. } => {
                let level = level as usize;
                if self.depth == 0
                    && self.quotes.is_empty()
                    && self.items.is_empty()
                    && !self.in_footnote
                {
                    // A marker a client can split the page on, so an outline can scroll
                    // to a heading.
                    self.push(HEADING_MARK);
                }
                self.push(&format!(
                    "<h{level} style=\"color:{};font-size:{}px;font-weight:600\">",
                    self.style.heading_colour(level),
                    self.style.heading_size(level)
                ));
                if self.style.marks {
                    let mark_size = if level == 1 {
                        self.style.heading_size(1)
                    } else {
                        self.style.size * 0.7
                    };
                    self.push(&format!(
                        "<span style=\"color:{};font-size:{}px;font-weight:400\">{}</span> ",
                        self.style.accent,
                        mark_size,
                        "#".repeat(level)
                    ));
                }
            }
            Tag::BlockQuote(_) => {
                self.quotes.push(QuoteState { opened: false });
            }
            Tag::CodeBlock(kind) => {
                // The code arrives as Text events; collect them into one block.
                self.code = Some(String::new());
                self.code_info = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::HtmlBlock => {}
            Tag::List(start) => {
                self.close_task_strike();
                match start {
                    Some(n) => self.push(&format!("<ol start=\"{n}\">")),
                    None => self.push("<ul>"),
                }
            }
            Tag::Item => {
                self.items.push(false);
                self.push("<li>");
            }
            Tag::FootnoteDefinition(label) => {
                let n = self.footnote_number(&label);
                self.in_footnote = true;
                self.footnote_head = Some((n, label.to_string()));
            }
            Tag::Table(aligns) => {
                self.table_aligns = aligns;
                self.push(&format!(
                    "<table cellspacing=\"0\" cellpadding=\"6\" border=\"1\" bordercolor=\"{}\" style=\"border-collapse:collapse\">",
                    self.style.line
                ));
            }
            Tag::TableHead => {
                self.in_head = true;
                self.cell = 0;
                self.push("<tr>");
            }
            Tag::TableRow => {
                self.cell = 0;
                self.push("<tr>");
            }
            Tag::TableCell => {
                let align = match self.table_aligns.get(self.cell) {
                    Some(Alignment::Center) => " align=\"center\"",
                    Some(Alignment::Right) => " align=\"right\"",
                    _ => "",
                };
                self.cell += 1;
                if self.in_head {
                    self.push(&format!("<th{align} bgcolor=\"{}\">", self.style.code_bg));
                } else {
                    self.push(&format!("<td{align}>"));
                }
            }
            Tag::Emphasis => {
                self.flush_hold();
                self.push("<i>");
            }
            Tag::Strong => {
                self.flush_hold();
                self.push("<b>");
            }
            Tag::Strikethrough => {
                self.flush_hold();
                self.push("<s>");
            }
            Tag::Superscript => self.push("<sup>"),
            Tag::Subscript => self.push("<sub>"),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                self.flush_hold();
                self.link(link_type, &dest_url);
            }
            Tag::Image {
                link_type,
                dest_url,
                ..
            } => {
                self.flush_hold();
                self.image = Some((dest_url.to_string(), link_type, String::new()));
            }
            Tag::MetadataBlock(_) => {
                self.skip = 1;
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {}
        }
    }

    fn link(&mut self, link_type: LinkType, dest: &str) {
        if let LinkType::WikiLink { has_pothole } = link_type {
            let (target, fragment) = split_fragment(dest);
            let target = normalise_target(target);
            let slug = if target.is_empty() {
                self.self_slug.clone()
            } else {
                self.resolver.resolve(&target)
            };
            self.links.push(LinkOut {
                target: target.clone(),
                slug: slug.clone(),
                alias: None,
                embed: false,
            });
            let _ = has_pothole;
            let (href, colour) = match slug {
                Some(s) => (
                    format!(
                        "rusty:page/{}{}",
                        s,
                        fragment.map(|f| format!("#{f}")).unwrap_or_default()
                    ),
                    self.style.link.clone(),
                ),
                None => (format!("rusty:new/{target}"), self.style.unresolved.clone()),
            };
            self.push(&format!(
                "<a href=\"{}\" style=\"color:{colour}\">",
                esc(&href)
            ));
            return;
        }
        let local = !dest.contains("://") && !dest.starts_with('#') && !dest.starts_with("mailto:");
        if local {
            let target = normalise_target(dest);
            if let Some(slug) = self.resolver.resolve(&target) {
                self.push(&format!(
                    "<a href=\"rusty:page/{}\" style=\"color:{}\">",
                    esc(&slug),
                    self.style.link
                ));
                return;
            }
        }
        self.push(&format!(
            "<a href=\"{}\" style=\"color:{}\">",
            esc(dest),
            self.style.link
        ));
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if self.hold_paragraph {
                    self.flush_hold();
                }
                if self.items.is_empty() {
                    self.push("</p>");
                } else {
                    self.item_break = true;
                }
            }
            TagEnd::Heading(level) => {
                self.push(&format!("</h{}>", level as usize));
                if self.style.marks && level as usize == 2 {
                    self.push(&format!("<hr style=\"color:{}\">", self.style.line));
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(q) = self.quotes.pop() {
                    if q.opened {
                        self.close_quote();
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.code_block(&code);
                }
            }
            TagEnd::HtmlBlock => {}
            TagEnd::List(ordered) => {
                self.push(if ordered { "</ol>" } else { "</ul>" });
            }
            TagEnd::Item => {
                self.close_task_strike();
                self.items.pop();
                self.item_break = false;
                self.push("</li>");
            }
            TagEnd::FootnoteDefinition => {
                if let Some((n, label)) = self.footnote_head.take() {
                    self.push(&format!(
                        "<p><a name=\"fn-{}\"></a><span style=\"color:{}\">{n}.</span></p>",
                        esc(&label),
                        self.style.muted
                    ));
                }
                self.in_footnote = false;
            }
            TagEnd::Table => self.push("</table>"),
            TagEnd::TableHead => {
                self.in_head = false;
                self.push("</tr>");
            }
            TagEnd::TableRow => self.push("</tr>"),
            TagEnd::TableCell => self.push(if self.in_head { "</th>" } else { "</td>" }),
            TagEnd::Emphasis => self.push("</i>"),
            TagEnd::Strong => self.push("</b>"),
            TagEnd::Strikethrough => self.push("</s>"),
            TagEnd::Superscript => self.push("</sup>"),
            TagEnd::Subscript => self.push("</sub>"),
            TagEnd::Link => self.push("</a>"),
            TagEnd::Image => {
                if let Some((dest, link_type, alt)) = self.image.take() {
                    self.image_end(&dest, link_type, &alt);
                }
            }
            TagEnd::MetadataBlock(_) => {}
            TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {}
        }
    }

    fn image_end(&mut self, dest: &str, link_type: LinkType, alt: &str) {
        let wiki = matches!(link_type, LinkType::WikiLink { .. });
        let (target, _) = split_fragment(dest);
        let target = normalise_target(target);
        // `![[img.png|300]]` sets a width in Obsidian.
        let width = alt.trim().parse::<u32>().ok().filter(|_| wiki);
        if wiki && !is_image(dest) && !looks_like_file(dest) {
            self.embed_page(&target, alt);
            return;
        }
        if wiki {
            self.links.push(LinkOut {
                target: target.clone(),
                slug: None,
                alias: None,
                embed: true,
            });
        }
        let src = if dest.contains("://") {
            dest.to_string()
        } else {
            self.resolver
                .file_url(dest)
                .unwrap_or_else(|| dest.to_string())
        };
        let width_attr = width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default();
        let alt_attr = if wiki {
            String::new()
        } else {
            format!(" alt=\"{}\"", esc(alt))
        };
        self.push(&format!(
            "<img src=\"{}\"{width_attr}{alt_attr}>",
            esc(&src)
        ));
    }

    fn embed_page(&mut self, target: &str, alias: &str) {
        let slug = self.resolver.resolve(target);
        self.links.push(LinkOut {
            target: target.to_string(),
            slug: slug.clone(),
            alias: Some(alias.to_string()).filter(|a| !a.is_empty()),
            embed: true,
        });
        let Some(slug) = slug else {
            self.push(&format!(
                "<a href=\"rusty:new/{}\" style=\"color:{}\">{}</a>",
                esc(target),
                self.style.unresolved,
                esc(target)
            ));
            return;
        };
        let bar = self.style.line.clone();
        if self.depth >= 2 || self.self_slug.as_deref() == Some(slug.as_str()) {
            self.push(&format!(
                "<a href=\"rusty:page/{}\" style=\"color:{}\">{}</a>",
                esc(&slug),
                self.style.link,
                esc(&slug)
            ));
            return;
        }
        let Some((title, raw)) = self.resolver.page(&slug) else {
            return;
        };
        let body = strip_comments(body_of(&raw));
        let mut nested = Writer::new(self.style, self.resolver, self.depth + 1, Some(&slug));
        nested.write_document(&body);
        let inner = nested.finish();
        self.push(&format!(
            "<table width=\"100%\" cellspacing=\"0\" cellpadding=\"0\"><tr><td width=\"3\" bgcolor=\"{bar}\"></td><td><table width=\"100%\" cellspacing=\"0\" cellpadding=\"8\"><tr><td><p><b><a href=\"rusty:page/{}\" style=\"color:{};text-decoration:none\">{}</a></b></p>{inner}</td></tr></table></td></tr></table>",
            esc(&slug),
            self.style.link,
            esc(&title)
        ));
    }

    fn text(&mut self, text: &str) {
        if let Some(code) = self.code.as_mut() {
            code.push_str(text);
            return;
        }
        if let Some((_, _, alt)) = self.image.as_mut() {
            alt.push_str(text);
            return;
        }
        if self.hold_paragraph {
            self.hold_paragraph = false;
            if let Some(q) = self.quotes.last_mut() {
                if !q.opened {
                    q.opened = true;
                    if let Some((kind, title, remainder)) = parse_callout(text) {
                        self.open_callout(&kind, &title);
                        let remainder = remainder.to_string();
                        self.push("<p style=\"margin-top:4px\">");
                        if !remainder.trim().is_empty() {
                            self.inline_text(&remainder);
                        } else {
                            // The body continues after the head's own line break.
                            self.skip_break = true;
                        }
                        return;
                    }
                    self.open_quote();
                }
            }
            self.push("<p>");
        }
        self.inline_text(text);
    }

    /// Escaped text with `==highlights==` and `#tags` turned into rich text.
    fn inline_text(&mut self, text: &str) {
        let escaped = esc(text);
        let with_marks = self.marks(&escaped);
        let with_tags = self.tags(&with_marks);
        self.push(&with_tags);
    }

    fn marks(&self, text: &str) -> String {
        let parts: Vec<&str> = text.split("==").collect();
        if parts.len() < 3 {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut open = false;
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            out.push_str(part);
            if i == last {
                break;
            }
            // A pair is only a highlight when it encloses something.
            if !open && parts.get(i + 1).is_some_and(|p| !p.is_empty()) && i + 1 < last {
                out.push_str(&format!(
                    "<span style=\"background-color:{}\">",
                    self.style.mark_bg
                ));
                open = true;
            } else if open {
                out.push_str("</span>");
                open = false;
            } else {
                out.push_str("==");
            }
        }
        if open {
            out.push_str("</span>");
        }
        out
    }

    fn tags(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(pos) = rest.find('#') {
            let before = &rest[..pos];
            let at_boundary = before
                .chars()
                .last()
                .is_none_or(|c| c.is_whitespace() || c == '(' || c == ',');
            let after = &rest[pos + 1..];
            let len = after
                .char_indices()
                .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/'))
                .map(|(i, _)| i)
                .unwrap_or(after.len());
            let tag = &after[..len];
            let valid = at_boundary && !tag.is_empty() && tag.chars().any(|c| c.is_alphabetic());
            out.push_str(before);
            if valid {
                out.push_str(&format!(
                    "<a href=\"rusty:tag/{tag}\" style=\"color:{};text-decoration:none\">#{tag}</a>",
                    self.style.tag
                ));
                rest = &after[len..];
            } else {
                out.push('#');
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }
}

/// Split `target#heading` or `target^block` into the target and the fragment.
pub(crate) fn split_fragment(dest: &str) -> (&str, Option<&str>) {
    match dest.find(['#', '^']) {
        Some(cut) => (
            &dest[..cut],
            Some(dest[cut + 1..].trim()).filter(|f| !f.is_empty()),
        ),
        None => (dest, None),
    }
}

/// A resolver over in-memory pages, for tests and for callers without a vault.
pub struct MapResolver {
    /// slug → (title, raw text)
    pub pages: HashMap<String, (String, String)>,
    /// file name → URL
    pub files: HashMap<String, String>,
}

impl Resolver for MapResolver {
    fn resolve(&self, target: &str) -> Option<String> {
        if self.pages.contains_key(target) {
            return Some(target.to_string());
        }
        let lower = target.to_lowercase();
        let mut matches = self.pages.keys().filter(|slug| {
            slug.to_lowercase() == lower
                || slug
                    .rsplit('/')
                    .next()
                    .is_some_and(|base| base.to_lowercase() == lower)
        });
        let first = matches.next()?.clone();
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }
    fn page(&self, slug: &str) -> Option<(String, String)> {
        self.pages.get(slug).cloned()
    }
    fn file_url(&self, target: &str) -> Option<String> {
        let name = target.rsplit('/').next()?;
        self.files.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> MapResolver {
        let mut pages = HashMap::new();
        pages.insert(
            "projects/orbit".to_string(),
            (
                "Orbit".to_string(),
                "---\ntitle: Orbit\ntype: project\n---\n\nA launcher.\n".to_string(),
            ),
        );
        pages.insert(
            "people/sarah-chen".to_string(),
            ("Sarah Chen".to_string(), "Sarah.\n".to_string()),
        );
        let mut files = HashMap::new();
        files.insert("pic.png".to_string(), "file:///vault/pic.png".to_string());
        MapResolver { pages, files }
    }

    fn html(body: &str) -> Rendered {
        render(body, &Style::default(), &resolver(), Some("concepts/here"))
    }

    #[test]
    fn wikilinks_resolve_alias_and_heading() {
        let r = html("See [[projects/orbit#Goals|the goals]] and [[sarah-chen]] and [[nope]].");
        assert!(r.html.contains(
            "<a href=\"rusty:page/projects/orbit#Goals\" style=\"color:#7aa2f7\">the goals</a>"
        ));
        assert!(r.html.contains("<a href=\"rusty:page/people/sarah-chen\""));
        assert!(r
            .html
            .contains("<a href=\"rusty:new/nope\" style=\"color:#565f89\">nope</a>"));
        assert_eq!(r.links.len(), 3);
        assert_eq!(r.unresolved, vec!["nope"]);
        assert_eq!(r.links[0].slug.as_deref(), Some("projects/orbit"));
    }

    #[test]
    fn embeds_render_pages_and_images() {
        let r = html("![[projects/orbit]]\n\n![[pic.png|300]]\n\n![[missing.png]]");
        assert!(r.html.contains("A launcher."));
        assert!(r.html.contains("rusty:page/projects/orbit"));
        assert!(r
            .html
            .contains("<img src=\"file:///vault/pic.png\" width=\"300\">"));
        assert!(r.html.contains("<img src=\"missing.png\">"));
        assert!(r.unresolved.is_empty(), "{:?}", r.unresolved);
        assert!(r.links.iter().all(|l| l.embed));
    }

    #[test]
    fn a_page_does_not_embed_itself_forever() {
        let mut res = resolver();
        res.pages.insert(
            "loop/a".to_string(),
            ("A".to_string(), "![[loop/a]] text".to_string()),
        );
        let r = render("![[loop/a]]", &Style::default(), &res, None);
        assert!(r.html.contains("text"));
        assert!(r.html.matches("<table").count() < 8);
    }

    #[test]
    fn callouts_and_quotes() {
        let r = html("> [!warning] Mind the gap\n> Second line.\n\n> plain quote");
        assert!(r.html.contains("bgcolor=\"#e0af68\""), "{}", r.html);
        assert!(r.html.contains("⚠&nbsp; Mind the gap"));
        assert!(r.html.contains("Second line."));
        assert!(!r.html.contains("[!warning]"));
        assert!(r
            .html
            .contains("bgcolor=\"#7aa2f7\"></td><td width=\"14\"></td><td>"));
        assert!(
            r.html.contains(
                "Mind the gap</span></b></p><p style=\"margin-top:4px\">Second line.</p>"
            ),
            "{}",
            r.html
        );
        let r = html("> [!tip]\n> Body");
        assert!(r.html.contains("✦&nbsp; Tip"));
        assert!(r.html.contains("Body"));
    }

    #[test]
    fn tasks_highlights_tags_and_comments() {
        let r = html("- [ ] open\n- [x] done ==really== #todo/now\n\nhidden %%comment%% here\n\n%%\nblock\n%%\n\n`#notatag` and #123 and a#b");
        assert!(r.html.contains("<a href=\"rusty:task/0\""));
        assert!(r.html.contains("<a href=\"rusty:task/1\""));
        assert!(r.html.contains("<s style=\"color:#787c99\">done"));
        assert!(r
            .html
            .contains("<span style=\"background-color:#4d4322\">really</span>"));
        assert!(r.html.contains("<a href=\"rusty:tag/todo/now\""));
        assert_eq!(r.tasks, 2);
        assert!(r.html.contains("hidden  here"));
        assert!(!r.html.contains("comment"));
        assert!(!r.html.contains("block"));
        assert!(r.html.contains("#notatag"));
        assert!(!r.html.contains("rusty:tag/notatag"));
        assert!(!r.html.contains("rusty:tag/123"));
        assert!(!r.html.contains("rusty:tag/b"));
    }

    #[test]
    fn code_is_left_alone() {
        let r = html("```rust\nlet x = [[not/link]]; // #nottag <b>\n```\n\ninline `[[x]] ==y==`");
        assert!(r.html.contains("[[not/link]]; // #nottag &lt;b&gt;"));
        assert!(r.html.contains("<pre style="));
        assert!(r.links.is_empty(), "{:?}", r.links);
        assert!(r.html.contains("[[x]] ==y=="));
        assert!(!r.html.contains("background-color:#4d4322"));
    }

    #[test]
    fn tables_footnotes_headings_and_breaks() {
        let r = html("# Title\n\n## Part *two*\n\nline one\nline two[^n]\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n[^n]: The note.\n\n---\n");
        assert_eq!(
            r.outline,
            vec![
                Heading {
                    level: 1,
                    text: "Title".into(),
                    line: 0
                },
                Heading {
                    level: 2,
                    text: "Part *two*".into(),
                    line: 2
                }
            ]
        );
        assert!(r.html.contains(
            "<!--h--><h1 style=\"color:#f7768e;font-size:28.832px;font-weight:600\">Title</h1>"
        ));
        assert_eq!(r.html.matches(HEADING_MARK).count(), 2);
        assert!(r.html.contains("line one<br>line two<sup>"));
        assert!(r.html.contains("<th bgcolor=\"#1f2335\">a</th>"));
        assert!(r.html.contains("<td align=\"right\">2</td>"));
        assert!(r.html.contains("<hr"));
        assert!(r.html.ends_with("The note.</p>"), "{}", r.html);
        assert_eq!(r.words, 24);
    }

    #[test]
    fn frontmatter_is_stripped_and_counted_out() {
        let raw = "---\ntitle: X\n---\n\nHello world.\n";
        let r = html(body_of(raw));
        assert!(!r.html.contains("title"));
        assert_eq!(r.words, 2);
        assert_eq!(r.characters, 14);
        assert_eq!(body_of("no fences"), "no fences");
    }

    #[test]
    fn marks_add_heading_prefixes_code_heads_and_task_colours() {
        let style = Style {
            marks: true,
            code_head: true,
            accent: "#ffb000".into(),
            alive: "#69d8bb".into(),
            line_bright: "#656b32".into(),
            ..Style::default()
        };
        let r = render(
            "# Title\n\n## Section\n\n> [!tip] Design directive\n> Keep it.\n\n```toml\na = 1\n```\n\n- [ ] open\n- [x] done\n",
            &style,
            &resolver(),
            None,
        );
        assert!(
            r.html
                .contains("color:#ffb000;font-size:28.832px;font-weight:400\">#</span> Title"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains("\">##</span> Section</h2><hr"),
            "{}",
            r.html
        );
        assert!(r.html.contains("DESIGN DIRECTIVE"), "{}", r.html);
        assert!(r.html.contains(">TOML</span>"), "{}", r.html);
        assert!(
            r.html.contains("color:#656b32\">☐</a>") && r.html.contains("color:#69d8bb\">☑</a>"),
            "{}",
            r.html
        );
        let plain = render(
            "## Section\n\n```toml\na = 1\n```\n",
            &Style::default(),
            &resolver(),
            None,
        );
        assert!(
            !plain.html.contains("##</span>") && !plain.html.contains("TOML"),
            "{}",
            plain.html
        );
    }

    #[test]
    fn style_fills_from_partial_json() {
        let s: Style = serde_json::from_str(r##"{"accent":"#000000"}"##).unwrap();
        assert_eq!(s.accent, "#000000");
        assert_eq!(s.text, Style::default().text);
        assert_eq!(s.headings.len(), 6);
    }
}
