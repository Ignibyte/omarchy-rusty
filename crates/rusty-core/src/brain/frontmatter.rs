//! YAML frontmatter parsing and serialization for brain pages.
//!
//! A brain page is YAML frontmatter between `---` fences, then the compiled truth, then
//! an optional `## Timeline` section that runs to the end of the file. Pages written
//! before 2026-09-02 marked the timeline with a bare `---` rule instead of the heading;
//! [`parse_page`] still reads that form when what follows the rule looks like timeline
//! entries, and `rusty-cli brain migrate` rewrites it. Rendering always writes the
//! heading form.

use std::collections::HashMap;

/// Structured YAML frontmatter for a brain page.
///
/// Known fields are typed explicitly. Arbitrary extra fields (e.g., `company`, `role`,
/// `relationship`) are captured in `extra` via `serde(flatten)` — this preserves
/// Obsidian-compatible frontmatter with user-defined properties.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BrainFrontmatter {
    /// Page title; empty when the file did not set one (then the file name stands in).
    #[serde(default)]
    pub title: String,
    /// Entity type (person, company, project, concept, meeting, idea, daily, inbox, or
    /// `note` for a page in any other folder); empty when the file did not set one.
    #[serde(rename = "type", default)]
    pub page_type: String,
    /// Alternative names for fuzzy resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Tags for categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// ISO date when the page was created.
    #[serde(default)]
    pub created: String,
    /// ISO date when the page was last updated.
    #[serde(default)]
    pub updated: String,
    /// Arbitrary extra fields (company, role, relationship, etc.).
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl BrainFrontmatter {
    /// Fill an empty title from the slug's file name and an empty type from the slug's
    /// top folder (`people/` is `person`, an unknown folder or the root is `note`).
    pub fn fill_defaults(&mut self, slug: &str) {
        if self.title.trim().is_empty() {
            self.title = slug.rsplit('/').next().unwrap_or(slug).to_string();
        }
        if self.page_type.trim().is_empty() {
            self.page_type = super::vault::type_for_slug(slug).to_string();
        }
    }

    /// Create new frontmatter with defaults for a given type and title.
    pub fn new(page_type: &str, title: &str) -> Self {
        let today = today_iso();
        Self {
            title: title.to_string(),
            page_type: page_type.to_string(),
            aliases: Vec::new(),
            tags: Vec::new(),
            created: today.clone(),
            updated: today,
            extra: HashMap::new(),
        }
    }
}

/// The heading that opens a page's timeline section.
pub const TIMELINE_HEADING: &str = "## Timeline";

/// Parsed sections of a brain page.
pub struct ParsedPage {
    /// YAML frontmatter.
    pub frontmatter: BrainFrontmatter,
    /// Content above the timeline (synthesized knowledge).
    pub compiled_truth: String,
    /// The timeline entries, without the `## Timeline` heading (append-only evidence).
    pub timeline: String,
}

/// Split raw page text into the frontmatter prefix (through the closing `---` line and
/// its newline) and the body after it. Errors when the fences are missing.
pub fn split_raw(raw: &str) -> Result<(&str, &str), String> {
    let lead = raw.len() - raw.trim_start().len();
    let trimmed = &raw[lead..];
    if !trimmed.starts_with("---") {
        return Err("Missing frontmatter: file must start with ---".to_string());
    }
    let after_open = &trimmed[3..];
    let after_open_lead = after_open.len() - after_open.trim_start_matches(['\r', '\n']).len();
    let after_open = &after_open[after_open_lead..];
    let close_pos = after_open
        .find("\n---")
        .ok_or_else(|| "Missing closing frontmatter delimiter (---)".to_string())?;
    // The body starts after the closing fence line; keep the newline that ends it in
    // the prefix so `prefix + body` reproduces the file.
    let after_close = &after_open[close_pos + 4..];
    let nl = after_close
        .find('\n')
        .map(|i| i + 1)
        .unwrap_or(after_close.len());
    let body_start = raw.len() - after_close.len() + nl;
    Ok((&raw[..body_start], &raw[body_start..]))
}

/// The YAML text between the fences.
fn yaml_of(prefix: &str) -> &str {
    let inner = prefix.trim_start().trim_start_matches("---");
    let inner = inner.trim_start_matches(['\r', '\n']);
    match inner.find("\n---") {
        Some(pos) => &inner[..pos],
        None => inner,
    }
}

/// Parse raw markdown content into frontmatter, compiled truth, and timeline. A file
/// without frontmatter fences is a page with empty frontmatter and everything as body;
/// only YAML that cannot be read is an error.
pub fn parse_page(raw: &str) -> Result<ParsedPage, String> {
    let (prefix, body) = match split_raw(raw) {
        Ok(parts) => parts,
        Err(_) => ("", raw),
    };
    let frontmatter: BrainFrontmatter = if prefix.is_empty() {
        BrainFrontmatter::default()
    } else {
        serde_yaml::from_str(yaml_of(prefix))
            .map_err(|e| format!("Failed to parse frontmatter: {e}"))?
    };
    let (compiled_truth, timeline) = split_body(body);
    Ok(ParsedPage {
        frontmatter,
        compiled_truth,
        timeline,
    })
}

/// Like [`parse_page`], but unreadable YAML becomes empty frontmatter with the whole file
/// as the body, so a page always opens.
pub fn parse_lenient(raw: &str) -> ParsedPage {
    match parse_page(raw) {
        Ok(parsed) => parsed,
        Err(_) => {
            let (compiled_truth, timeline) = split_body(raw);
            ParsedPage {
                frontmatter: BrainFrontmatter::default(),
                compiled_truth,
                timeline,
            }
        }
    }
}

/// The frontmatter as ordered `(key, value)` pairs, the way the file lists them, for a
/// properties view. Empty when there is no frontmatter or it cannot be read.
pub fn properties_of(raw: &str) -> Vec<(String, serde_json::Value)> {
    let Ok((prefix, _)) = split_raw(raw) else {
        return Vec::new();
    };
    let Ok(map) = serde_yaml::from_str::<serde_yaml::Mapping>(yaml_of(prefix)) else {
        return Vec::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| {
            let key = match k {
                serde_yaml::Value::String(s) => s,
                other => serde_yaml::to_string(&other).ok()?.trim().to_string(),
            };
            let value = serde_json::to_value(v).ok()?;
            Some((key, value))
        })
        .collect()
}

/// Byte offset of the `## Timeline` heading line, when the body has one.
fn find_heading(body: &str) -> Option<usize> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == TIMELINE_HEADING {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Whether text after a bare `---` rule is a legacy timeline: its first line is a
/// timeline bullet, a dated heading, or the timeline heading itself.
fn looks_like_timeline(rest: &str) -> bool {
    match rest
        .lines()
        .map(str::trim_end)
        .find(|l| !l.trim().is_empty())
    {
        Some(first) => {
            first.starts_with("- **") || first.starts_with("### ") || first == TIMELINE_HEADING
        }
        None => false,
    }
}

/// Drop a leading `## Timeline` heading line from timeline text.
fn strip_heading(text: &str) -> &str {
    let t = text.trim_start_matches(['\r', '\n']);
    match t.split_once('\n') {
        Some((first, rest)) if first.trim_end() == TIMELINE_HEADING => rest,
        None if t.trim_end() == TIMELINE_HEADING => "",
        _ => t,
    }
}

/// Drop a trailing bare `---` rule from compiled truth (the legacy separator).
fn strip_trailing_rule(truth: &str) -> &str {
    let t = truth.trim_end();
    if t == "---" {
        return "";
    }
    t.strip_suffix("\n---").unwrap_or(t)
}

/// Split a page body into compiled truth and timeline entries.
pub fn split_body(body: &str) -> (String, String) {
    if let Some(pos) = find_heading(body) {
        let truth = strip_trailing_rule(&body[..pos]).trim().to_string();
        let after = body[pos..]
            .split_once('\n')
            .map(|(_, rest)| rest)
            .unwrap_or("");
        return (truth, after.trim().to_string());
    }
    if let Some(sep_pos) = body.find("\n---\n") {
        let rest = &body[sep_pos + 5..];
        if looks_like_timeline(rest) {
            let truth = body[..sep_pos].trim().to_string();
            return (truth, strip_heading(rest).trim().to_string());
        }
    }
    (body.trim().to_string(), String::new())
}

/// Whether a body still uses the pre-2026-09 bare `---` rule for its timeline.
pub fn uses_legacy_rule(body: &str) -> bool {
    if find_heading(body).is_some() {
        // A heading preceded by the old rule is legacy too.
        return body
            .find("\n---\n")
            .is_some_and(|sep| find_heading(body).is_some_and(|h| sep < h));
    }
    body.find("\n---\n")
        .is_some_and(|sep| looks_like_timeline(&body[sep + 5..]))
}

/// Render the part of a page after the frontmatter: a blank line, the compiled truth,
/// and the `## Timeline` section when there are entries.
pub fn render_body(compiled_truth: &str, timeline: &str) -> String {
    let truth = compiled_truth.trim();
    let timeline = strip_heading(timeline).trim();
    let mut body = String::new();
    if !truth.is_empty() {
        body.push('\n');
        body.push_str(truth);
        body.push('\n');
    }
    if !timeline.is_empty() {
        body.push('\n');
        body.push_str(TIMELINE_HEADING);
        body.push_str("\n\n");
        body.push_str(timeline);
        body.push('\n');
    }
    body
}

/// Render structured page parts back to a full markdown string.
pub fn render_page(
    frontmatter: &BrainFrontmatter,
    compiled_truth: &str,
    timeline: &str,
) -> Result<String, String> {
    let yaml =
        serde_yaml::to_string(frontmatter).map_err(|e| format!("Failed to serialize YAML: {e}"))?;
    // Wrap YAML in --- delimiters (serde_yaml does not add them)
    let yaml_clean = yaml.trim_end().trim_start_matches("---").trim();
    Ok(format!(
        "---\n{yaml_clean}\n---\n{}",
        render_body(compiled_truth, timeline)
    ))
}

/// The frontmatter of a raw page as an ordered YAML mapping (empty when there is none
/// or it cannot be read), the body after the closing fence, and whether fences existed.
fn mapping_and_body(raw: &str) -> Result<(serde_yaml::Mapping, &str, bool), String> {
    match split_raw(raw) {
        Ok((prefix, body)) => {
            let text = yaml_of(prefix);
            if text.trim().is_empty() {
                return Ok((serde_yaml::Mapping::new(), body, true));
            }
            let mapping: serde_yaml::Mapping = serde_yaml::from_str(text)
                .map_err(|e| format!("Failed to parse frontmatter: {e}"))?;
            Ok((mapping, body, true))
        }
        Err(_) => Ok((serde_yaml::Mapping::new(), raw, false)),
    }
}

/// The raw page for a mapping and a body: the fences and the YAML, then the body byte
/// for byte; no frontmatter at all when the mapping is empty.
fn assemble(mapping: &serde_yaml::Mapping, body: &str, had_fences: bool) -> Result<String, String> {
    if mapping.is_empty() {
        return Ok(body.trim_start_matches('\n').to_string());
    }
    let yaml =
        serde_yaml::to_string(mapping).map_err(|e| format!("Failed to serialize YAML: {e}"))?;
    let yaml = yaml
        .trim_end()
        .trim_start_matches("---")
        .trim_start_matches('\n');
    let separator = if had_fences || body.starts_with('\n') || body.is_empty() {
        ""
    } else {
        "\n"
    };
    Ok(format!("---\n{yaml}\n---\n{separator}{body}"))
}

/// Set one frontmatter key to a JSON value (text, number, checkbox, date as text, or a
/// list), keeping the other keys in their order and the body byte for byte. A page
/// without frontmatter gains it.
pub fn set_property(raw: &str, key: &str, value: serde_json::Value) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("A property needs a key".to_string());
    }
    let (mut mapping, body, had_fences) = mapping_and_body(raw)?;
    let value = serde_yaml::to_value(value).map_err(|e| format!("Bad property value: {e}"))?;
    mapping.insert(serde_yaml::Value::String(key.to_string()), value);
    assemble(&mapping, body, had_fences)
}

/// Remove one frontmatter key, keeping the rest in order and the body byte for byte.
pub fn remove_property(raw: &str, key: &str) -> Result<String, String> {
    let (mut mapping, body, had_fences) = mapping_and_body(raw)?;
    mapping.shift_remove(serde_yaml::Value::String(key.trim().to_string()));
    assemble(&mapping, body, had_fences)
}

/// Set one top-level key in the frontmatter text itself: the line that holds the key is
/// replaced, or a line is added before the closing fence, and every other byte of the
/// file is kept, comments, quoting and unknown keys included (TICKET-046). `Ok(None)`
/// when the page has no frontmatter. A key whose value spans several lines (a list, a
/// block scalar) goes through [`set_property`] instead.
pub fn set_scalar_line(raw: &str, key: &str, value: &str) -> Result<Option<String>, String> {
    let Ok((prefix, body)) = split_raw(raw) else {
        return Ok(None);
    };
    let scalar = serde_yaml::to_string(&serde_yaml::Value::String(value.to_string()))
        .map_err(|e| format!("Failed to serialize YAML: {e}"))?;
    let scalar = scalar.trim_end();
    let mapping_write = || set_property(raw, key, serde_json::Value::String(value.to_string()));
    if scalar.contains('\n') {
        return mapping_write().map(Some);
    }
    let lines: Vec<&str> = prefix.split_inclusive('\n').collect();
    let Some(open) = lines.iter().position(|l| l.trim_start().starts_with("---")) else {
        return mapping_write().map(Some);
    };
    let Some(close) = (open + 1..lines.len()).find(|&i| lines[i].starts_with("---")) else {
        return mapping_write().map(Some);
    };
    let holds_key = |line: &str| {
        line.strip_prefix(key)
            .and_then(|rest| rest.strip_prefix(':'))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']))
    };
    let mut out = String::with_capacity(raw.len() + key.len() + scalar.len());
    match (open + 1..close).find(|&i| holds_key(lines[i])) {
        Some(i) => {
            let rest = lines[i][key.len() + 1..].trim();
            let continues = i + 1 < close && lines[i + 1].starts_with([' ', '\t', '-']);
            if rest.is_empty() || rest.starts_with(['|', '>']) || continues {
                return mapping_write().map(Some);
            }
            let eol = if lines[i].ends_with("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            for (n, line) in lines.iter().enumerate() {
                if n == i {
                    out.push_str(&format!("{key}: {scalar}{eol}"));
                } else {
                    out.push_str(line);
                }
            }
        }
        None => {
            let eol = if lines[close].ends_with("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            for (n, line) in lines.iter().enumerate() {
                if n == close {
                    out.push_str(&format!("{key}: {scalar}{eol}"));
                }
                out.push_str(line);
            }
        }
    }
    // A key the line match missed (`title : x`) would now be in the mapping twice, and
    // YAML that read before the edit must still read after it; either way the mapping
    // writer does the job instead.
    let readable = |text: &str| serde_yaml::from_str::<serde_yaml::Mapping>(text).is_ok();
    if readable(yaml_of(prefix)) {
        let edited = split_raw(&out).map(|(p, _)| yaml_of(p)).unwrap_or_default();
        if !readable(edited) {
            return mapping_write().map(Some);
        }
    }
    out.push_str(body);
    Ok(Some(out))
}

/// The page with everything after the frontmatter replaced by `body`; the frontmatter is
/// kept byte for byte. A page without frontmatter is `body` alone, without the blank
/// line a body opens with after a fence.
pub fn replace_body(raw: &str, body: &str) -> String {
    match split_raw(raw) {
        Ok((prefix, _)) => format!("{prefix}{body}"),
        Err(_) => body.trim_start_matches('\n').to_string(),
    }
}

/// Today's date on the machine's local day (`TZ`, else `/etc/localtime`), as
/// `YYYY-MM-DD`. Every date the brain writes comes from here (TICKET-044).
pub(crate) fn today_iso() -> String {
    day_of(&chrono::Local::now())
}

/// The calendar day of `now` in its own zone, as `YYYY-MM-DD`.
pub(crate) fn day_of<Tz: chrono::TimeZone>(now: &chrono::DateTime<Tz>) -> String {
    now.date_naive().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_is_the_local_day() {
        let local_day = || chrono::Local::now().format("%Y-%m-%d").to_string();
        let before = local_day();
        let today = today_iso();
        let after = local_day();
        assert!(
            today == before || today == after,
            "{today} vs {before}/{after}"
        );
    }

    const HAND_WRITTEN: &str = "---\n# kept by hand\ntitle: launch\nzeta: \"quoted\"\nalpha: 1   # odd spacing\nupdated: 2026-01-01\ntags:\n  - x\n---\n\nBody.\n";

    #[test]
    fn set_scalar_line_changes_one_line() {
        let next = set_scalar_line(HAND_WRITTEN, "title", "liftoff")
            .unwrap()
            .unwrap();
        assert_eq!(
            next,
            HAND_WRITTEN.replace("title: launch\n", "title: liftoff\n")
        );
        let next = set_scalar_line(HAND_WRITTEN, "updated", "2026-10-04")
            .unwrap()
            .unwrap();
        assert_eq!(
            next,
            HAND_WRITTEN.replace("updated: 2026-01-01\n", "updated: 2026-10-04\n")
        );
    }

    #[test]
    fn set_scalar_line_adds_a_missing_key_before_the_fence() {
        let raw = "---\ntitle: A\n---\nBody\n";
        let next = set_scalar_line(raw, "updated", "2026-10-04")
            .unwrap()
            .unwrap();
        assert_eq!(next, "---\ntitle: A\nupdated: 2026-10-04\n---\nBody\n");
        let comment_only = set_scalar_line("---\n# none yet\n---\nBody\n", "title", "A")
            .unwrap()
            .unwrap();
        assert_eq!(comment_only, "---\n# none yet\ntitle: A\n---\nBody\n");
    }

    #[test]
    fn set_scalar_line_matches_the_whole_key_and_keeps_crlf() {
        let raw = "---\r\ntitle2: other\r\ntitle: A\r\n---\r\nBody\r\n";
        let next = set_scalar_line(raw, "title", "B").unwrap().unwrap();
        assert_eq!(next, "---\r\ntitle2: other\r\ntitle: B\r\n---\r\nBody\r\n");
    }

    #[test]
    fn set_scalar_line_quotes_when_yaml_needs_it() {
        let raw = "---\ntitle: A\n---\n";
        let next = set_scalar_line(raw, "title", "yes: really")
            .unwrap()
            .unwrap();
        let props = properties_of(&next);
        assert_eq!(props[0].1, serde_json::json!("yes: really"));
    }

    #[test]
    fn set_scalar_line_never_writes_a_key_twice() {
        let raw = "---\ntitle : A\nkept: 1\n---\nBody\n";
        let next = set_scalar_line(raw, "title", "B").unwrap().unwrap();
        let props = properties_of(&next);
        assert_eq!(props.len(), 2, "{next}");
        assert!(props.contains(&("title".to_string(), serde_json::json!("B"))));
    }

    #[test]
    fn set_scalar_line_falls_back_for_a_list() {
        let next = set_scalar_line(HAND_WRITTEN, "tags", "solo")
            .unwrap()
            .unwrap();
        let props = properties_of(&next);
        let tags = props.iter().find(|(k, _)| k == "tags").unwrap();
        assert_eq!(tags.1, serde_json::json!("solo"));
        assert!(next.ends_with("\nBody.\n"));
    }

    #[test]
    fn set_scalar_line_and_replace_body_leave_a_page_without_frontmatter() {
        assert_eq!(set_scalar_line("Just text.\n", "title", "A").unwrap(), None);
        assert_eq!(replace_body("Just text.\n", "\nNew.\n"), "New.\n");
        assert_eq!(
            replace_body(HAND_WRITTEN, "\nNew.\n"),
            HAND_WRITTEN.replace("\nBody.\n", "\nNew.\n")
        );
    }

    #[test]
    fn day_of_reads_the_zone_not_utc() {
        use chrono::TimeZone;
        // 23:30 on 2 October at UTC−5 is already 3 October in UTC.
        let utc = chrono::Utc.with_ymd_and_hms(2026, 10, 3, 4, 30, 0).unwrap();
        let west = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(day_of(&utc), "2026-10-03");
        assert_eq!(day_of(&utc.with_timezone(&west)), "2026-10-02");
    }

    #[test]
    fn parse_page_with_timeline_section() {
        let raw = "---\ntitle: Sarah Chen\ntype: person\ntags:\n  - engineering\n---\n\nShe is a CTO.\n\n## Timeline\n\n- **2026-04-12** (meeting) — Met at conference.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.frontmatter.title, "Sarah Chen");
        assert_eq!(parsed.frontmatter.page_type, "person");
        assert_eq!(parsed.frontmatter.tags, vec!["engineering"]);
        assert_eq!(parsed.compiled_truth, "She is a CTO.");
        assert_eq!(
            parsed.timeline,
            "- **2026-04-12** (meeting) — Met at conference."
        );
    }

    #[test]
    fn parse_legacy_rule_before_bullets() {
        let raw = "---\ntitle: Orbit\ntype: project\n---\n\nA launcher.\n\n---\n\n- **2026-04-12** — Kicked off.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.compiled_truth, "A launcher.");
        assert_eq!(parsed.timeline, "- **2026-04-12** — Kicked off.");
        let (_, body) = split_raw(raw).unwrap();
        assert!(uses_legacy_rule(body));
    }

    #[test]
    fn parse_legacy_rule_with_heading() {
        let raw = "---\ntitle: Sarah Chen\ntype: person\n---\n\nShe is a CTO.\n\n---\n\n## Timeline\n\n### 2026-04-12\nMet at conference.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.compiled_truth, "She is a CTO.");
        assert_eq!(parsed.timeline, "### 2026-04-12\nMet at conference.");
        let (_, body) = split_raw(raw).unwrap();
        assert!(uses_legacy_rule(body));
    }

    #[test]
    fn horizontal_rule_in_prose_stays_in_the_truth() {
        let raw = "---\ntitle: Essay\ntype: concept\n---\n\nPart one.\n\n---\n\nPart two.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.compiled_truth, "Part one.\n\n---\n\nPart two.");
        assert!(parsed.timeline.is_empty());
        let (_, body) = split_raw(raw).unwrap();
        assert!(!uses_legacy_rule(body));
    }

    #[test]
    fn parse_no_timeline() {
        let raw = "---\ntitle: Rust\ntype: concept\n---\n\nA systems programming language.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.compiled_truth, "A systems programming language.");
        assert!(parsed.timeline.is_empty());
    }

    #[test]
    fn parse_empty_body() {
        let raw = "---\ntitle: Empty\ntype: inbox\n---\n";
        let parsed = parse_page(raw).unwrap();
        assert!(parsed.compiled_truth.is_empty());
        assert!(parsed.timeline.is_empty());
    }

    #[test]
    fn split_raw_reproduces_the_file() {
        let raw = "---\ntitle: Empty\ntype: inbox\n---\n\nBody.\n";
        let (prefix, body) = split_raw(raw).unwrap();
        assert_eq!(prefix, "---\ntitle: Empty\ntype: inbox\n---\n");
        assert_eq!(body, "\nBody.\n");
        assert_eq!(format!("{prefix}{body}"), raw);
    }

    #[test]
    fn parse_extra_fields_preserved() {
        let raw =
            "---\ntitle: Sarah Chen\ntype: person\ncompany: Acme\nrole: CTO\n---\n\nContent.\n";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(
            parsed
                .frontmatter
                .extra
                .get("company")
                .and_then(|v| v.as_str()),
            Some("Acme")
        );
        assert_eq!(
            parsed
                .frontmatter
                .extra
                .get("role")
                .and_then(|v| v.as_str()),
            Some("CTO")
        );
    }

    #[test]
    fn render_round_trip_writes_the_heading_form() {
        let fm = BrainFrontmatter {
            title: "Test Page".to_string(),
            page_type: "concept".to_string(),
            aliases: vec!["test".to_string()],
            tags: vec!["testing".to_string()],
            created: "2026-04-12".to_string(),
            updated: "2026-04-12".to_string(),
            extra: HashMap::new(),
        };
        let truth = "This is compiled truth.";
        let timeline = "- **2026-04-12** — Created.";
        let rendered = render_page(&fm, truth, timeline).unwrap();
        assert!(rendered.contains("\n## Timeline\n\n- **2026-04-12** — Created.\n"));
        // Only the frontmatter's closing fence is a `---` line.
        assert_eq!(rendered.matches("\n---\n").count(), 1);
        let parsed = parse_page(&rendered).unwrap();
        assert_eq!(parsed.frontmatter.title, "Test Page");
        assert_eq!(parsed.frontmatter.aliases, vec!["test"]);
        assert_eq!(parsed.compiled_truth, truth);
        assert_eq!(parsed.timeline, timeline);
    }

    #[test]
    fn render_does_not_duplicate_a_heading_in_the_timeline() {
        let body = render_body("Truth.", "## Timeline\n\n- **2026-04-12** — x");
        assert_eq!(body.matches(TIMELINE_HEADING).count(), 1);
        assert_eq!(body, "\nTruth.\n\n## Timeline\n\n- **2026-04-12** — x\n");
        assert_eq!(render_body("", ""), "");
    }

    #[test]
    fn parse_missing_frontmatter_is_a_page_with_defaults() {
        let raw = "No frontmatter here.";
        let parsed = parse_page(raw).unwrap();
        assert_eq!(parsed.compiled_truth, "No frontmatter here.");
        assert!(parsed.frontmatter.title.is_empty());
        let mut fm = parsed.frontmatter;
        fm.fill_defaults("2026-09-02");
        assert_eq!(fm.title, "2026-09-02");
        assert_eq!(fm.page_type, "note");
        let mut fm = BrainFrontmatter::default();
        fm.fill_defaults("people/sarah-chen");
        assert_eq!(fm.title, "sarah-chen");
        assert_eq!(fm.page_type, "person");
    }

    #[test]
    fn unreadable_yaml_is_lenient_and_strict_errors() {
        let raw = "---\ntitle: [unclosed\n---\n\nBody.\n";
        assert!(parse_page(raw).is_err());
        let parsed = parse_lenient(raw);
        assert!(parsed.frontmatter.title.is_empty());
        assert!(parsed.compiled_truth.contains("Body."));
    }

    #[test]
    fn property_edits_keep_order_and_body() {
        let raw = "---\ntitle: T\ntype: note\nrole: CTO\ntags:\n  - a\n---\n\nBody with  odd   spacing.\n\n## Timeline\n\n- **2026-09-03** — x\n";
        let set = set_property(raw, "role", serde_json::json!("CEO")).unwrap();
        let keys: Vec<String> = properties_of(&set).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["title", "type", "role", "tags"]);
        assert!(
            set.ends_with(
                "\n---\n\nBody with  odd   spacing.\n\n## Timeline\n\n- **2026-09-03** — x\n"
            ),
            "{set}"
        );
        let added = set_property(&set, "done", serde_json::json!(true)).unwrap();
        assert!(properties_of(&added)
            .iter()
            .any(|(k, v)| k == "done" && v == &serde_json::json!(true)));
        let listed = set_property(&added, "tags", serde_json::json!(["a", "b/c"])).unwrap();
        assert_eq!(properties_of(&listed)[3].1, serde_json::json!(["a", "b/c"]));
        let removed = remove_property(&listed, "role").unwrap();
        let keys: Vec<String> = properties_of(&removed)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, vec!["title", "type", "tags", "done"]);
        assert!(set_property(raw, " ", serde_json::json!(1)).is_err());
        // A page without frontmatter gains it; removing the last key drops it again.
        let bare = set_property("Just text.\n", "status", serde_json::json!("open")).unwrap();
        assert_eq!(bare, "---\nstatus: open\n---\n\nJust text.\n");
        assert_eq!(remove_property(&bare, "status").unwrap(), "Just text.\n");
    }

    #[test]
    fn properties_keep_the_file_order() {
        let raw = "---\ntitle: T\ntype: note\nrole: CTO\ntags:\n  - a\n---\n\nBody.\n";
        let props = properties_of(raw);
        let keys: Vec<&str> = props.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["title", "type", "role", "tags"]);
        assert_eq!(props[3].1, serde_json::json!(["a"]));
        assert!(properties_of("no fences").is_empty());
    }
}
