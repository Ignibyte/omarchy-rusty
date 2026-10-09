---
type: "Reference"
title: "Markdown rendering: Obsidian's flavour to Qt rich text"
openwiki_generated: true
sources:
  - id: openwiki-source-8237034823c8af0c3889096c
    resource: repo://crates/rusty-core/src/brain/blocks.rs
  - id: openwiki-source-b0fccdb632d2710022a80345
    resource: repo://crates/rusty-core/src/brain/render.rs
generated: {by: "claude-code", at: "2026-10-09T17:29:16.068Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T17:29:16.068Z
---

# Markdown rendering: Obsidian's flavour to Qt rich text

## Purpose

A client shows a page the way Obsidian's reading view does, without a web engine. One
renderer in the core turns Obsidian-flavoured markdown into the HTML subset Qt's rich
text engine understands, and the same parser builds typed blocks for a client that draws
its own, as Marley does.

## Ownership

- `crates/rusty-core/src/brain/render.rs`: `render(body, &Style, &dyn Resolver,
  self_slug)` on pulldown-cmark 0.13 with tables, footnotes, strikethrough, task lists,
  wikilinks and math enabled; `Style` (colours, fonts and the base size, the skin's
  roles the page is painted with, and two switches, `marks` and `code_head`, every
  field with a default so a partial JSON fills the rest); the `Resolver` trait (link targets to
  slugs, page text for embeds, file URLs for images); `Rendered` (html, outline, links,
  unresolved targets, task count, word and character counts).
- `crates/rusty-core/src/brain/blocks.rs`: the typed blocks (below).

## Runtime flow

1. `BrainManager::render_page` strips the frontmatter (`body_of`), builds a
   `DbResolver` over the vault and the index, and calls `render`.
2. Two pre-passes: `%% comments %%` are stripped outside fenced code, and every
   `> [!kind]` head is rewritten with private-use markers, because pulldown-cmark
   splits `[!kind]` into several text events.
3. The writer walks the events and emits HTML with inline styles: headings with
   Obsidian's size scale and the theme's heading colours (with `marks`, a `#` per
   level in the accent before the text and a rule under a section title), callouts
   and quotes as tables with a coloured bar (with `marks`, the label uppercase and
   small), code blocks as tables with the code background (with `code_head`, a header
   strip naming the fenced language), task boxes in the line colour and the alive
   colour when `marks` is on, tables with
   header cells, footnotes collected after an `<hr>`, task boxes as
   `rusty:task/<n>` links (done items struck through), `==highlights==` as background
   spans, `#tags` as `rusty:tag/<tag>` links, images with `file://` URLs from the
   resolver, page embeds rendered inline to a depth of two and never into themselves.
4. Wikilinks resolve through the resolver: a page becomes `rusty:page/<slug>[#frag]` in
   the link colour; a missing page becomes `rusty:new/<target>` in the unresolved colour.
5. A marker (`<!--h-->`) precedes every top-level heading so a client can split the
   reading view into blocks and scroll the outline to one.
6. The client routes the `rusty:` links: page navigation, page creation, task toggling
   by index in the raw source, tag search.

### Blocks for clients that cannot draw rich text (TICKET-036)

`brain_render` with `blocks: true` adds the body as typed blocks (`brain/blocks.rs`):
headings, paragraphs, lists whose items may be tasks (`{done, index}`, the index a
client toggles by), code, quotes, callouts (kind, title, fold `open`/`closed`), tables,
rules, raw HTML and footnotes, holding inline runs (text, strong, emphasis, strike,
highlight, code, math, links, wikilinks with target, heading and slug or none, tags,
images with a loadable `src` and width, page embeds expanded one level, footnote
references, breaks). The builder runs the same parser and options over a copy of the
body masked byte for byte: a comment becomes spaces of its own length and a callout's
`[!` and `]` become markers of the same byte length, so each block's `range` is an
offset into the body and `body_start` places the body in the file; a top-level
heading block starts at each heading line outside fenced code. The HTML writer is not touched, and an
answer without the flag is unchanged.

## Invariants

- The renderer needs no Qt; every construct has a unit test.
- Colours are parameters: a client sends its theme's tokens as the `style` argument of
  `brain_render`; without one the renderer's defaults apply.
- Code, fenced or inline, is never scanned for links, tags or highlights.
- Task indexes in the HTML count the same items the source-side toggle counts
  (list markers, including inside quotes; never inside fences).

## Failure modes

- Rich text has no stylesheet, so a construct without an inline style falls back to
  the `Text` item's font and colour.
- Anchors inside a page (`#footnote`, `[[page#heading]]`) render but do not scroll yet.

## Extension points

- A new construct: handle its event in `Writer::event` and add a test in `render.rs`;
  give it a block kind in `blocks.rs` when a block client needs to draw it.
- A new style token: add it to `Style` with a default, so a client that does not send it
  still renders.

## Tests

- `cargo test -p rusty-core brain::render`, `brain::blocks` and `brain::links`.

## Primary sources

- `crates/rusty-core/src/brain/render.rs`, `crates/rusty-core/src/brain/blocks.rs`,
  `crates/rusty-core/src/brain/links.rs`
