//! A page as data: typed blocks and inline runs, with wikilinks resolved, embeds expanded
//! one level and callouts recognised, each block carrying its byte range in the page's
//! body (TICKET-036). A client that cannot draw Qt rich text (Marley's GPUI) draws these.
//!
//! The builder reads the same Obsidian flavour as [`super::render`] with the same parser,
//! over a masked copy of the body that keeps every byte where it was: a `%% comment %%`
//! becomes spaces of its own length (newlines kept), and a quote's `[!kind]` head swaps
//! `[!` and `]` for markers of the same byte length. Ranges are therefore offsets into the
//! body the caller passed, the coordinates `page_sections` splits by. The HTML path is not
//! touched.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::links::normalise_target;
use super::render::{body_of, is_image, looks_like_file, split_fragment, Resolver};

/// Replaces `[!` in a callout head: U+0080, two bytes like the two it replaces.
const CALLOUT_OPEN: char = '\u{80}';
/// Replaces the `]` that closes a callout's kind: U+0001, one byte.
const CALLOUT_CLOSE: char = '\u{1}';

/// A byte range in the body, `[start, end)`.
pub type Range = [usize; 2];

/// One block of a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// `#` to `######`.
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        range: Range,
    },
    Paragraph {
        inlines: Vec<Inline>,
        range: Range,
    },
    /// A bulleted or numbered list; `start` is the first number of a numbered one.
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
        range: Range,
    },
    /// Fenced or indented code; `lang` is the fence's first word, empty when none.
    Code {
        lang: String,
        text: String,
        range: Range,
    },
    /// A plain blockquote.
    Quote {
        blocks: Vec<Block>,
        range: Range,
    },
    /// An Obsidian callout: `> [!kind]± Title`. `fold` is `open` (`+`), `closed` (`-`)
    /// or absent; the title defaults to the kind, capitalised.
    Callout {
        kind: String,
        title: String,
        fold: Option<String>,
        blocks: Vec<Block>,
        range: Range,
    },
    /// `aligns` per column: `left`, `center`, `right` or `none`.
    Table {
        aligns: Vec<String>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        range: Range,
    },
    Rule {
        range: Range,
    },
    /// Raw HTML, passed through as written.
    Html {
        html: String,
        range: Range,
    },
    /// A footnote's definition.
    Footnote {
        label: String,
        number: usize,
        blocks: Vec<Block>,
        range: Range,
    },
}

/// One item of a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    /// Present when the item is a task (`- [ ]`, `- [x]`).
    pub task: Option<Task>,
    pub blocks: Vec<Block>,
    pub range: Range,
}

/// A task item's state and its index among the page's tasks, the index the tools toggle
/// by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub done: bool,
    pub index: usize,
}

/// A run inside a block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text {
        text: String,
    },
    Strong {
        children: Vec<Inline>,
    },
    Emphasis {
        children: Vec<Inline>,
    },
    Strike {
        children: Vec<Inline>,
    },
    /// `==marked==`.
    Highlight {
        children: Vec<Inline>,
    },
    Superscript {
        children: Vec<Inline>,
    },
    Subscript {
        children: Vec<Inline>,
    },
    Code {
        text: String,
    },
    /// `$…$`, or `$$…$$` when `display`.
    Math {
        text: String,
        display: bool,
    },
    /// A markdown link; `slug` when it points at a page in the vault.
    Link {
        url: String,
        slug: Option<String>,
        children: Vec<Inline>,
    },
    /// `[[target#heading|shown]]`; `slug` is absent when it resolves to no page.
    Wikilink {
        target: String,
        heading: Option<String>,
        slug: Option<String>,
        children: Vec<Inline>,
    },
    /// `#tag`, without the `#`.
    Tag {
        tag: String,
    },
    /// An image, markdown or `![[file.png|300]]`; `src` is a URL a client can load.
    Image {
        src: String,
        alt: String,
        width: Option<u32>,
    },
    /// `![[page]]`: the page's blocks when it resolves, one level deep and never itself.
    Embed {
        target: String,
        slug: Option<String>,
        title: Option<String>,
        blocks: Vec<Block>,
    },
    FootnoteRef {
        label: String,
        number: usize,
    },
    /// A line break; `hard` for a hard one.
    Break {
        hard: bool,
    },
    Html {
        html: String,
    },
}

/// The blocks of a page body (frontmatter already removed). `self_slug` is the page's
/// own slug, so it never embeds itself.
pub fn build(body: &str, resolver: &dyn Resolver, self_slug: Option<&str>) -> Vec<Block> {
    build_at(body, resolver, self_slug, 0)
}

fn build_at(
    body: &str,
    resolver: &dyn Resolver,
    self_slug: Option<&str>,
    depth: usize,
) -> Vec<Block> {
    let masked = mask(body);
    let mut builder = Builder {
        resolver,
        self_slug: self_slug.map(str::to_string),
        depth,
        stack: vec![Open::Doc(Vec::new())],
        tasks: 0,
        footnotes: Vec::new(),
        skip: 0,
        skip_break: false,
        alt_spans: 0,
    };
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_MATH;
    for (event, range) in Parser::new_ext(&masked, options).into_offset_iter() {
        builder.event(event, [range.start, range.end]);
    }
    builder.finish()
}

/// The body with comments blanked and callout heads marked, byte for byte the same
/// length, so every offset into it is an offset into `body`.
fn mask(body: &str) -> String {
    let mut bytes = body.as_bytes().to_vec();
    // Comments, outside fenced code, spanning lines or not.
    let mut in_fence = false;
    let mut in_comment = false;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !in_comment && (trimmed.starts_with("```") || trimmed.starts_with("~~~")) {
            in_fence = !in_fence;
        } else if !in_fence {
            let mut i = 0;
            loop {
                let found = line[i..].find("%%");
                let (from, to) = match (in_comment, found) {
                    (true, Some(p)) => {
                        in_comment = false;
                        (i, i + p + 2)
                    }
                    (true, None) => (i, line.len()),
                    (false, Some(p)) => {
                        in_comment = true;
                        (i + p, i + p + 2)
                    }
                    (false, None) => break,
                };
                for b in &mut bytes[offset + from..offset + to] {
                    if *b != b'\n' {
                        *b = b' ';
                    }
                }
                i = to;
                if i >= line.len() {
                    break;
                }
            }
        }
        offset += line.len();
    }
    // Callout heads: `> [!kind]` on a quote line outside fenced code.
    let mut in_fence = false;
    let mut offset = 0;
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| body.to_string());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence && trimmed.starts_with('>') {
            let after = trimmed[1..].trim_start();
            if let Some(rest) = after.strip_prefix("[!") {
                if let Some(close) = rest.find(']') {
                    let kind = &rest[..close];
                    let valid = !kind.is_empty()
                        && kind
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
                    if valid {
                        let open_at = offset + line.len() - after.len();
                        let close_at = open_at + 2 + close;
                        let mut marker = [0u8; 2];
                        CALLOUT_OPEN.encode_utf8(&mut marker);
                        bytes[open_at..open_at + 2].copy_from_slice(&marker);
                        bytes[close_at] = CALLOUT_CLOSE as u8;
                    }
                }
            }
        }
        offset += line.len();
    }
    String::from_utf8(bytes).unwrap_or_else(|_| body.to_string())
}

/// What a run of inlines is being gathered into.
enum Span {
    Strong,
    Emphasis,
    Strike,
    Superscript,
    Subscript,
    Link {
        url: String,
        slug: Option<String>,
    },
    Wikilink {
        target: String,
        heading: Option<String>,
        slug: Option<String>,
    },
}

/// An element the builder is inside.
enum Open {
    Doc(Vec<Block>),
    Quote {
        range: Range,
        blocks: Vec<Block>,
        callout: Option<(String, String, Option<String>)>,
        /// The first text of the quote has not been read yet.
        fresh: bool,
    },
    List {
        range: Range,
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Item {
        range: Range,
        task: Option<Task>,
        blocks: Vec<Block>,
    },
    Footnote {
        range: Range,
        label: String,
        number: usize,
        blocks: Vec<Block>,
    },
    Table {
        range: Range,
        aligns: Vec<String>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Row {
        head: bool,
        cells: Vec<Vec<Inline>>,
    },
    Cell(Vec<Inline>),
    Para {
        range: Range,
        inlines: Vec<Inline>,
        /// Opened by inline content with no paragraph around it (a tight list item).
        implicit: bool,
    },
    Heading {
        range: Range,
        level: u8,
        inlines: Vec<Inline>,
    },
    Span(Span, Vec<Inline>),
    Image {
        dest: String,
        wiki: bool,
        alt: String,
    },
    Code {
        range: Range,
        lang: String,
        text: String,
    },
    Html {
        range: Range,
        html: String,
    },
}

struct Builder<'a> {
    resolver: &'a dyn Resolver,
    self_slug: Option<String>,
    depth: usize,
    stack: Vec<Open>,
    tasks: usize,
    footnotes: Vec<String>,
    /// Inside a metadata block: events are skipped until it closes.
    skip: usize,
    /// The break after a callout's head line belongs to the head.
    skip_break: bool,
    /// Formatting spans open inside an image's alt text.
    alt_spans: usize,
}

impl Builder<'_> {
    fn finish(mut self) -> Vec<Block> {
        self.close_implicit();
        while self.stack.len() > 1 {
            // Malformed nesting cannot come out of the parser; close what is open.
            let open = self.stack.pop();
            if let Some(Open::Para { range, inlines, .. }) = open {
                self.push_block(Block::Paragraph { inlines, range });
            }
        }
        match self.stack.pop() {
            Some(Open::Doc(blocks)) => blocks,
            _ => Vec::new(),
        }
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        match self.footnotes.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.footnotes.push(label.to_string());
                self.footnotes.len()
            }
        }
    }

    /// Add a block to the nearest element that holds blocks.
    fn push_block(&mut self, block: Block) {
        for open in self.stack.iter_mut().rev() {
            match open {
                Open::Doc(blocks)
                | Open::Quote { blocks, .. }
                | Open::Item { blocks, .. }
                | Open::Footnote { blocks, .. } => {
                    blocks.push(block);
                    return;
                }
                _ => {}
            }
        }
    }

    /// Whether the innermost element takes inlines.
    fn takes_inlines(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(Open::Para { .. } | Open::Heading { .. } | Open::Span(..) | Open::Cell(_))
        )
    }

    /// Add an inline to the innermost element that takes them, opening a paragraph when
    /// the content has none around it.
    fn push_inline(&mut self, inline: Inline, range: Range) {
        self.ensure_inlines(range);
        match self.stack.last_mut() {
            Some(Open::Para {
                inlines,
                range: para,
                implicit,
            }) => {
                if *implicit {
                    para[1] = para[1].max(range[1]);
                }
                inlines.push(inline);
            }
            Some(Open::Heading { inlines, .. })
            | Some(Open::Span(_, inlines))
            | Some(Open::Cell(inlines)) => inlines.push(inline),
            _ => {}
        }
    }

    /// Close a paragraph opened for loose inline content before a block starts or a
    /// container ends.
    fn close_implicit(&mut self) {
        if let Some(Open::Para { implicit: true, .. }) = self.stack.last() {
            if let Some(Open::Para { range, inlines, .. }) = self.stack.pop() {
                self.push_block(Block::Paragraph { inlines, range });
            }
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range) {
        if self.skip > 0 {
            match event {
                Event::Start(_) => self.skip += 1,
                Event::End(_) => self.skip -= 1,
                _ => {}
            }
            return;
        }
        // Formatting inside an image's alt text is flattened into the alt.
        if matches!(self.stack.last(), Some(Open::Image { .. })) {
            match event {
                Event::Start(_) => {
                    self.alt_spans += 1;
                    return;
                }
                Event::End(_) if self.alt_spans > 0 => {
                    self.alt_spans -= 1;
                    return;
                }
                Event::Code(code) => {
                    if let Some(Open::Image { alt, .. }) = self.stack.last_mut() {
                        alt.push_str(&code);
                    }
                    return;
                }
                _ => {}
            }
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text, range),
            Event::Code(code) => self.push_inline(
                Inline::Code {
                    text: code.to_string(),
                },
                range,
            ),
            Event::InlineMath(m) => self.push_inline(
                Inline::Math {
                    text: m.to_string(),
                    display: false,
                },
                range,
            ),
            Event::DisplayMath(m) => self.push_inline(
                Inline::Math {
                    text: m.to_string(),
                    display: true,
                },
                range,
            ),
            Event::Html(h) | Event::InlineHtml(h) => {
                if let Some(Open::Html { html, .. }) = self.stack.last_mut() {
                    html.push_str(&h);
                } else {
                    self.push_inline(
                        Inline::Html {
                            html: h.to_string(),
                        },
                        range,
                    );
                }
            }
            Event::FootnoteReference(label) => {
                let number = self.footnote_number(&label);
                self.push_inline(
                    Inline::FootnoteRef {
                        label: label.to_string(),
                        number,
                    },
                    range,
                );
            }
            Event::SoftBreak | Event::HardBreak => {
                if self.skip_break {
                    self.skip_break = false;
                    return;
                }
                let hard = matches!(event, Event::HardBreak);
                self.push_inline(Inline::Break { hard }, range);
            }
            Event::Rule => {
                self.close_implicit();
                self.push_block(Block::Rule { range });
            }
            Event::TaskListMarker(done) => {
                let index = self.tasks;
                self.tasks += 1;
                if let Some(Open::Item { task, .. }) = self.stack.last_mut() {
                    *task = Some(Task { done, index });
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range) {
        let block_level = matches!(
            tag,
            Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::HtmlBlock
                | Tag::List(_)
                | Tag::FootnoteDefinition(_)
                | Tag::Table(_)
        );
        if block_level {
            self.close_implicit();
        }
        let open = match tag {
            Tag::Paragraph => Open::Para {
                range,
                inlines: Vec::new(),
                implicit: false,
            },
            Tag::Heading { level, .. } => Open::Heading {
                range,
                level: level as u8,
                inlines: Vec::new(),
            },
            Tag::BlockQuote(_) => Open::Quote {
                range,
                blocks: Vec::new(),
                callout: None,
                fresh: true,
            },
            Tag::CodeBlock(kind) => Open::Code {
                range,
                lang: match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                },
                text: String::new(),
            },
            Tag::HtmlBlock => Open::Html {
                range,
                html: String::new(),
            },
            Tag::List(start) => Open::List {
                range,
                ordered: start.is_some(),
                start,
                items: Vec::new(),
            },
            Tag::Item => Open::Item {
                range,
                task: None,
                blocks: Vec::new(),
            },
            Tag::FootnoteDefinition(label) => {
                let number = self.footnote_number(&label);
                Open::Footnote {
                    range,
                    label: label.to_string(),
                    number,
                    blocks: Vec::new(),
                }
            }
            Tag::Table(aligns) => Open::Table {
                range,
                aligns: aligns
                    .iter()
                    .map(|a| {
                        match a {
                            Alignment::Left => "left",
                            Alignment::Center => "center",
                            Alignment::Right => "right",
                            Alignment::None => "none",
                        }
                        .to_string()
                    })
                    .collect(),
                head: Vec::new(),
                rows: Vec::new(),
            },
            Tag::TableHead => Open::Row {
                head: true,
                cells: Vec::new(),
            },
            Tag::TableRow => Open::Row {
                head: false,
                cells: Vec::new(),
            },
            Tag::TableCell => Open::Cell(Vec::new()),
            Tag::Emphasis => self.span(Span::Emphasis, range),
            Tag::Strong => self.span(Span::Strong, range),
            Tag::Strikethrough => self.span(Span::Strike, range),
            Tag::Superscript => self.span(Span::Superscript, range),
            Tag::Subscript => self.span(Span::Subscript, range),
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let span = self.link_span(link_type, &dest_url);
                self.span(span, range)
            }
            Tag::Image {
                link_type,
                dest_url,
                ..
            } => {
                self.ensure_inlines(range);
                Open::Image {
                    dest: dest_url.to_string(),
                    wiki: matches!(link_type, LinkType::WikiLink { .. }),
                    alt: String::new(),
                }
            }
            Tag::MetadataBlock(_) => {
                self.skip = 1;
                return;
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                return;
            }
        };
        self.stack.push(open);
    }

    /// Open a span, making sure it sits inside something that takes inlines.
    fn span(&mut self, span: Span, range: Range) -> Open {
        self.ensure_inlines(range);
        Open::Span(span, Vec::new())
    }

    /// Open a paragraph for inline content that has none around it (a tight list item).
    fn ensure_inlines(&mut self, range: Range) {
        if !self.takes_inlines() {
            self.stack.push(Open::Para {
                range,
                inlines: Vec::new(),
                implicit: true,
            });
        }
    }

    fn link_span(&self, link_type: LinkType, dest: &str) -> Span {
        if let LinkType::WikiLink { .. } = link_type {
            let (target, fragment) = split_fragment(dest);
            let target = normalise_target(target);
            let slug = if target.is_empty() {
                self.self_slug.clone()
            } else {
                self.resolver.resolve(&target)
            };
            return Span::Wikilink {
                target,
                heading: fragment.map(str::to_string),
                slug,
            };
        }
        let local = !dest.contains("://") && !dest.starts_with('#') && !dest.starts_with("mailto:");
        let slug = local
            .then(|| self.resolver.resolve(&normalise_target(dest)))
            .flatten();
        Span::Link {
            url: dest.to_string(),
            slug,
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => return,
            TagEnd::Item | TagEnd::BlockQuote(_) | TagEnd::FootnoteDefinition => {
                self.close_implicit();
            }
            _ => {}
        }
        let Some(open) = self.stack.pop() else {
            return;
        };
        match open {
            Open::Para { range, inlines, .. } => {
                // A callout whose head was its whole first paragraph leaves it empty.
                let empty = inlines
                    .iter()
                    .all(|i| matches!(i, Inline::Text { text } if text.trim().is_empty()));
                if !empty {
                    self.push_block(Block::Paragraph { inlines, range });
                }
            }
            Open::Heading {
                range,
                level,
                inlines,
            } => self.push_block(Block::Heading {
                level,
                inlines,
                range,
            }),
            Open::Quote {
                range,
                blocks,
                callout,
                ..
            } => self.push_block(match callout {
                Some((kind, title, fold)) => Block::Callout {
                    kind,
                    title,
                    fold,
                    blocks,
                    range,
                },
                None => Block::Quote { blocks, range },
            }),
            Open::Code { range, lang, text } => self.push_block(Block::Code { lang, text, range }),
            Open::Html { range, html } => self.push_block(Block::Html { html, range }),
            Open::List {
                range,
                ordered,
                start,
                items,
            } => self.push_block(Block::List {
                ordered,
                start,
                items,
                range,
            }),
            Open::Item {
                range,
                task,
                blocks,
            } => {
                if let Some(Open::List { items, .. }) = self.stack.last_mut() {
                    items.push(ListItem {
                        task,
                        blocks,
                        range,
                    });
                }
            }
            Open::Footnote {
                range,
                label,
                number,
                blocks,
            } => self.push_block(Block::Footnote {
                label,
                number,
                blocks,
                range,
            }),
            Open::Table {
                range,
                aligns,
                head,
                rows,
            } => self.push_block(Block::Table {
                aligns,
                head,
                rows,
                range,
            }),
            Open::Row { head, cells } => {
                if let Some(Open::Table {
                    head: table_head,
                    rows,
                    ..
                }) = self.stack.last_mut()
                {
                    if head {
                        *table_head = cells;
                    } else {
                        rows.push(cells);
                    }
                }
            }
            Open::Cell(inlines) => {
                if let Some(Open::Row { cells, .. }) = self.stack.last_mut() {
                    cells.push(inlines);
                }
            }
            Open::Span(span, children) => {
                let inline = match span {
                    Span::Strong => Inline::Strong { children },
                    Span::Emphasis => Inline::Emphasis { children },
                    Span::Strike => Inline::Strike { children },
                    Span::Superscript => Inline::Superscript { children },
                    Span::Subscript => Inline::Subscript { children },
                    Span::Link { url, slug } => Inline::Link {
                        url,
                        slug,
                        children,
                    },
                    Span::Wikilink {
                        target,
                        heading,
                        slug,
                    } => Inline::Wikilink {
                        target,
                        heading,
                        slug,
                        children,
                    },
                };
                self.push_inline(inline, [0, 0]);
            }
            Open::Image { dest, wiki, alt } => {
                let inline = self.image(&dest, wiki, &alt);
                self.push_inline(inline, [0, 0]);
            }
            Open::Doc(blocks) => {
                // Never closed by an event; put it back.
                self.stack.push(Open::Doc(blocks));
            }
        }
    }

    fn image(&self, dest: &str, wiki: bool, alt: &str) -> Inline {
        let (target, _) = split_fragment(dest);
        let target = normalise_target(target);
        if wiki && !is_image(dest) && !looks_like_file(dest) {
            let slug = self.resolver.resolve(&target);
            let (title, blocks) = match &slug {
                Some(s) if self.depth == 0 && self.self_slug.as_deref() != Some(s.as_str()) => {
                    match self.resolver.page(s) {
                        Some((title, raw)) => (
                            Some(title),
                            build_at(body_of(&raw), self.resolver, Some(s), self.depth + 1),
                        ),
                        None => (None, Vec::new()),
                    }
                }
                Some(s) => (self.resolver.page(s).map(|(t, _)| t), Vec::new()),
                None => (None, Vec::new()),
            };
            return Inline::Embed {
                target,
                slug,
                title,
                blocks,
            };
        }
        let src = if dest.contains("://") {
            dest.to_string()
        } else {
            self.resolver
                .file_url(dest)
                .unwrap_or_else(|| dest.to_string())
        };
        // `![[img.png|300]]` sets a width in Obsidian; a markdown image's text is its alt.
        let width = alt.trim().parse::<u32>().ok().filter(|_| wiki);
        Inline::Image {
            src,
            alt: if wiki && width.is_some() {
                String::new()
            } else {
                alt.to_string()
            },
            width,
        }
    }

    fn text(&mut self, text: &str, range: Range) {
        match self.stack.last_mut() {
            Some(Open::Code { text: code, .. }) => {
                code.push_str(text);
                return;
            }
            Some(Open::Image { alt, .. }) => {
                alt.push_str(text);
                return;
            }
            Some(Open::Html { html, .. }) => {
                html.push_str(text);
                return;
            }
            _ => {}
        }
        // The first text of a quote decides whether it is a callout.
        let quote = self
            .stack
            .iter_mut()
            .rev()
            .find(|o| matches!(o, Open::Quote { .. }));
        if let Some(Open::Quote { fresh, callout, .. }) = quote {
            if *fresh {
                *fresh = false;
                if let Some((kind, title, fold, rest)) = parse_head(text) {
                    *callout = Some((kind, title, fold));
                    if rest.trim().is_empty() {
                        self.skip_break = true;
                    } else {
                        for inline in split_text(rest) {
                            self.push_inline(inline, range);
                        }
                    }
                    return;
                }
            }
        }
        for inline in split_text(text) {
            self.push_inline(inline, range);
        }
    }
}

/// A callout head as masked: the marker, the kind, the marker, `+` or `-`, the title to
/// the end of the line. The kind capitalised when the title is empty.
fn parse_head(text: &str) -> Option<(String, String, Option<String>, &str)> {
    let rest = text.strip_prefix(CALLOUT_OPEN)?;
    let close = rest.find(CALLOUT_CLOSE)?;
    let kind = rest[..close].trim().to_lowercase();
    if kind.is_empty() {
        return None;
    }
    let mut after = &rest[close + CALLOUT_CLOSE.len_utf8()..];
    let fold = match after.chars().next() {
        Some('+') => Some("open".to_string()),
        Some('-') => Some("closed".to_string()),
        _ => None,
    };
    if fold.is_some() {
        after = &after[1..];
    }
    let (title, remainder) = after.split_once('\n').unwrap_or((after, ""));
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
    Some((kind, title, fold, remainder))
}

/// Text split into plain runs, `==highlights==` and `#tags`.
fn split_text(text: &str) -> Vec<Inline> {
    let parts: Vec<&str> = text.split("==").collect();
    if parts.len() < 3 {
        return tags_in(text);
    }
    let mut out = Vec::new();
    let mut plain = String::new();
    let last = parts.len() - 1;
    let mut i = 0;
    while i <= last {
        plain.push_str(parts[i]);
        // A pair is a highlight only when it encloses something and closes.
        if i + 2 <= last && !parts[i + 1].is_empty() {
            out.extend(tags_in(&std::mem::take(&mut plain)));
            out.push(Inline::Highlight {
                children: tags_in(parts[i + 1]),
            });
            i += 2;
        } else {
            if i < last {
                plain.push_str("==");
            }
            i += 1;
        }
    }
    out.extend(tags_in(&plain));
    out
}

/// Text split into plain runs and `#tags`, by the renderer's rule: a `#` at the start or
/// after a space, `(` or `,`, then letters, digits, `_`, `-` or `/`, at least one a letter.
fn tags_in(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find('#') {
        let before = &rest[..pos];
        let at_boundary = before
            .chars()
            .last()
            .or_else(|| plain.chars().last())
            .is_none_or(|c| c.is_whitespace() || c == '(' || c == ',');
        let after = &rest[pos + 1..];
        let len = after
            .char_indices()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/'))
            .map(|(i, _)| i)
            .unwrap_or(after.len());
        let tag = &after[..len];
        plain.push_str(before);
        if at_boundary && !tag.is_empty() && tag.chars().any(|c| c.is_alphabetic()) {
            if !plain.is_empty() {
                out.push(Inline::Text {
                    text: std::mem::take(&mut plain),
                });
            }
            out.push(Inline::Tag {
                tag: tag.to_string(),
            });
            rest = &after[len..];
        } else {
            plain.push('#');
            rest = after;
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        out.push(Inline::Text { text: plain });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::render::{render, MapResolver, Style};
    use std::collections::HashMap;

    fn resolver() -> MapResolver {
        let mut pages = HashMap::new();
        pages.insert(
            "projects/orbit".to_string(),
            (
                "Orbit".to_string(),
                "---\ntitle: Orbit\n---\n\nA launcher. ![[people/sarah-chen]]\n".to_string(),
            ),
        );
        pages.insert(
            "people/sarah-chen".to_string(),
            ("Sarah Chen".to_string(), "Sarah.\n".to_string()),
        );
        pages.insert(
            "concepts/here".to_string(),
            (
                "Here".to_string(),
                "Me again: ![[concepts/here]]\n".to_string(),
            ),
        );
        let mut files = HashMap::new();
        files.insert("pic.png".to_string(), "file:///vault/pic.png".to_string());
        MapResolver { pages, files }
    }

    const PAGE: &str = "# Title\n\nSome **bold** and *it* and ~~gone~~ and `code` and ==marked== with #tag/nested.\nA [[projects/orbit#Goals|the goals]] link, a [[nope]] one, [web](https://example.com).\n\n## Lists\n\n- [ ] open task\n- [x] done task\n  - nested\n\n1. one\n2. two\n\n> plain quote\n\n> [!warning]- Careful now\n> body of the callout %% hidden %%\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|:--|--:|\n| 1 | 2 |\n\n---\n\n![[pic.png|300]] ![[projects/orbit]] footnote[^n]\n\n[^n]: The note.\n\n%% a comment\nover lines %%\n\n### Last\n";

    fn kinds(blocks: &[Block]) -> Vec<&'static str> {
        blocks
            .iter()
            .map(|b| match b {
                Block::Heading { .. } => "heading",
                Block::Paragraph { .. } => "paragraph",
                Block::List { .. } => "list",
                Block::Code { .. } => "code",
                Block::Quote { .. } => "quote",
                Block::Callout { .. } => "callout",
                Block::Table { .. } => "table",
                Block::Rule { .. } => "rule",
                Block::Html { .. } => "html",
                Block::Footnote { .. } => "footnote",
            })
            .collect()
    }

    fn flatten<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Inline>) {
        for inline in inlines {
            out.push(inline);
            match inline {
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Strike { children }
                | Inline::Highlight { children }
                | Inline::Superscript { children }
                | Inline::Subscript { children }
                | Inline::Link { children, .. }
                | Inline::Wikilink { children, .. } => flatten(children, out),
                _ => {}
            }
        }
    }

    fn all_inlines(blocks: &[Block]) -> Vec<&Inline> {
        let mut out = Vec::new();
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    flatten(inlines, &mut out)
                }
                Block::List { items, .. } => {
                    for item in items {
                        out.extend(all_inlines(&item.blocks));
                    }
                }
                Block::Quote { blocks, .. }
                | Block::Callout { blocks, .. }
                | Block::Footnote { blocks, .. } => out.extend(all_inlines(blocks)),
                Block::Table { head, rows, .. } => {
                    for cell in head.iter().chain(rows.iter().flatten()) {
                        flatten(cell, &mut out);
                    }
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn every_block_kind() {
        let blocks = build(PAGE, &resolver(), Some("concepts/here"));
        assert_eq!(
            kinds(&blocks),
            [
                "heading",
                "paragraph",
                "heading",
                "list",
                "list",
                "quote",
                "callout",
                "code",
                "table",
                "rule",
                "paragraph",
                "footnote",
                "heading"
            ],
            "{blocks:#?}"
        );
        let Block::List { items, ordered, .. } = &blocks[3] else {
            panic!()
        };
        assert!(!ordered);
        assert_eq!(
            items[0].task,
            Some(Task {
                done: false,
                index: 0
            })
        );
        assert_eq!(
            items[1].task,
            Some(Task {
                done: true,
                index: 1
            })
        );
        assert!(matches!(items[1].blocks.last(), Some(Block::List { .. })));
        let Block::List { ordered, start, .. } = &blocks[4] else {
            panic!()
        };
        assert!(*ordered && *start == Some(1));
        let Block::Code { lang, text, .. } = &blocks[7] else {
            panic!()
        };
        assert_eq!((lang.as_str(), text.as_str()), ("rust", "fn main() {}\n"));
        let Block::Table {
            aligns, head, rows, ..
        } = &blocks[8]
        else {
            panic!()
        };
        assert_eq!(aligns, &["left", "right"]);
        assert_eq!((head.len(), rows.len()), (2, 1));
        let inlines = all_inlines(&blocks);
        let has = |f: &dyn Fn(&Inline) -> bool| inlines.iter().any(|i| f(i));
        assert!(has(&|i| matches!(i, Inline::Strong { .. })));
        assert!(has(&|i| matches!(i, Inline::Emphasis { .. })));
        assert!(has(&|i| matches!(i, Inline::Strike { .. })));
        assert!(has(
            &|i| matches!(i, Inline::Code { text } if text == "code")
        ));
        assert!(has(&|i| matches!(i, Inline::Highlight { .. })));
        assert!(has(
            &|i| matches!(i, Inline::Tag { tag } if tag == "tag/nested")
        ));
        assert!(has(
            &|i| matches!(i, Inline::Link { url, .. } if url == "https://example.com")
        ));
        assert!(has(
            &|i| matches!(i, Inline::Image { src, width: Some(300), .. } if src == "file:///vault/pic.png")
        ));
        assert!(has(&|i| matches!(i, Inline::FootnoteRef { number: 1, .. })));
        // Ranges are offsets into the body.
        let Block::Heading { range, .. } = &blocks[0] else {
            panic!()
        };
        assert_eq!(&PAGE[range[0]..range[1]].trim_end(), &"# Title");
    }

    #[test]
    fn callouts_and_comments() {
        let blocks = build(PAGE, &resolver(), None);
        let Block::Callout {
            kind,
            title,
            fold,
            blocks: body,
            ..
        } = &blocks[6]
        else {
            panic!("{:#?}", blocks[6]);
        };
        assert_eq!(kind, "warning");
        assert_eq!(title, "Careful now");
        assert_eq!(fold.as_deref(), Some("closed"));
        let text: String = all_inlines(body)
            .iter()
            .filter_map(|i| match i {
                Inline::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(text.contains("body of the callout"), "{text:?}");
        let json = serde_json::to_string(&blocks).unwrap();
        assert!(
            !json.contains("hidden") && !json.contains("a comment"),
            "{json}"
        );
        let Block::Quote { .. } = &blocks[5] else {
            panic!("a plain quote stays a quote");
        };
        let untitled = build("> [!tip]\n> Body.\n", &resolver(), None);
        let Block::Callout { title, fold, .. } = &untitled[0] else {
            panic!("{untitled:#?}")
        };
        assert_eq!((title.as_str(), fold), ("Tip", &None));
    }

    #[test]
    fn wikilinks_match_the_link_list() {
        let blocks = build(PAGE, &resolver(), Some("concepts/here"));
        let rendered = render(PAGE, &Style::default(), &resolver(), Some("concepts/here"));
        let mine: Vec<(String, Option<String>)> = all_inlines(&blocks)
            .iter()
            .filter_map(|i| match i {
                Inline::Wikilink { target, slug, .. } | Inline::Embed { target, slug, .. } => {
                    Some((target.clone(), slug.clone()))
                }
                _ => None,
            })
            .collect();
        let theirs: Vec<(String, Option<String>)> = rendered
            .links
            .iter()
            .filter(|l| !(l.embed && l.slug.is_none() && l.target.contains('.')))
            .map(|l| (l.target.clone(), l.slug.clone()))
            .collect();
        assert_eq!(mine, theirs);
        let goals = all_inlines(&blocks)
            .into_iter()
            .find(|i| matches!(i, Inline::Wikilink { target, .. } if target == "projects/orbit"))
            .unwrap();
        let Inline::Wikilink {
            heading, children, ..
        } = goals
        else {
            panic!()
        };
        assert_eq!(heading.as_deref(), Some("Goals"));
        assert_eq!(
            children,
            &vec![Inline::Text {
                text: "the goals".into()
            }]
        );
        let unresolved: Vec<&str> = all_inlines(&blocks)
            .iter()
            .filter_map(|i| match i {
                Inline::Wikilink {
                    target, slug: None, ..
                } => Some(target.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(unresolved, rendered.unresolved);
    }

    #[test]
    fn embeds_go_one_level_and_never_self() {
        let blocks = build(PAGE, &resolver(), Some("concepts/here"));
        let embed = all_inlines(&blocks)
            .into_iter()
            .find(|i| matches!(i, Inline::Embed { .. }))
            .unwrap();
        let Inline::Embed {
            slug,
            title,
            blocks: inner,
            ..
        } = embed
        else {
            panic!()
        };
        assert_eq!(slug.as_deref(), Some("projects/orbit"));
        assert_eq!(title.as_deref(), Some("Orbit"));
        // Orbit's own embed of Sarah is named but not expanded: one level.
        let nested = all_inlines(inner)
            .into_iter()
            .find(|i| matches!(i, Inline::Embed { .. }))
            .unwrap();
        let Inline::Embed { slug, blocks, .. } = nested else {
            panic!()
        };
        assert_eq!(slug.as_deref(), Some("people/sarah-chen"));
        assert!(blocks.is_empty());
        let me = build("![[concepts/here]]\n", &resolver(), Some("concepts/here"));
        let Some(Inline::Embed { blocks, .. }) = all_inlines(&me).into_iter().next() else {
            panic!("{me:#?}")
        };
        assert!(blocks.is_empty(), "a page never embeds itself");
    }

    #[test]
    fn heading_ranges_start_the_sections() {
        // A client that splits a page into sections starts one at every heading line
        // outside fenced code; each such line is where a top-level heading block starts.
        let body =
            "Intro line.\n\n# One\ntext\n```\n# not a heading\n```\n## Two\n> quote\n### Three\n";
        let mut starts = Vec::new();
        let mut in_fence = false;
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            let t = line.trim_start();
            if t.starts_with("```") {
                in_fence = !in_fence;
            } else if !in_fence && line.starts_with('#') {
                starts.push(offset);
            }
            offset += line.len();
        }
        let blocks = build(body, &resolver(), None);
        let heads: Vec<usize> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading { range, .. } => Some(range[0]),
                _ => None,
            })
            .collect();
        assert_eq!(heads, starts);
        // Masking keeps offsets: a comment and a callout before a heading move nothing.
        let masked = "%% gone %%\n> [!note] Hi\n> there\n\n# After\n";
        let blocks = build(masked, &resolver(), None);
        let Some(Block::Heading { range, .. }) = blocks.last() else {
            panic!("{blocks:#?}")
        };
        assert_eq!(&masked[range[0]..range[1]].trim_end(), &"# After");
    }
}
