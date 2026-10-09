//! The brain loop, Ask, Decide, Follow up (TICKET-018). A consultation before a
//! decision, a `decision` page linked to what it rested on, and a follow-up that says how
//! it went. The pages are the truth; `brain_consultations` in the database is the receipt
//! of the ask, with the question, the hits and the outcome.

use super::frontmatter::{properties_of, render_page, today_iso};
use super::semantic::Embedder;
use super::{BrainManager, BrainPage, BrainSearchResult};

/// The page type and its folder.
pub const DECISION_TYPE: &str = "decision";
/// What a follow-up may set.
pub const FOLLOW_UP_STATUSES: &[&str] = &["kept", "revised", "superseded"];
/// The source written on the timeline entries the loop adds.
const SOURCE: &str = "decision";

/// One decision, as the lists and the view show it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DecisionSummary {
    pub slug: String,
    pub title: String,
    pub question: String,
    pub status: String,
    pub decided: String,
    pub follow_up_by: String,
    pub overdue: bool,
    /// The day of the last follow-up, empty before the first (TICKET-048).
    #[serde(default)]
    pub followed_up: String,
    /// The decision that replaced this one, empty unless it was superseded.
    #[serde(default)]
    pub superseded_by: String,
}

/// What `ask` hands back: the consultation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Consultation {
    /// The id `decide` and `no_decision` refer to.
    pub id: String,
    pub question: String,
    /// Ranked pages, decisions excluded (they come next with their status).
    pub pages: Vec<BrainSearchResult>,
    /// The decisions that touch the question.
    pub decisions: Vec<DecisionSummary>,
    /// The follow-ups due today or overdue.
    pub due: Vec<DecisionSummary>,
}

/// What `decide` needs.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Decide {
    pub consultation: String,
    pub title: String,
    pub choice: String,
    pub rationale: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub follow_up_by: Option<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
}

/// What `follow_up` needs.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct FollowUp {
    pub slug: String,
    pub outcome: String,
    pub status: String,
    #[serde(default)]
    pub successor: Option<String>,
    #[serde(default)]
    pub follow_up_by: Option<String>,
}

/// What `due` hands back.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Due {
    pub due: Vec<DecisionSummary>,
    pub all: Vec<DecisionSummary>,
}

/// `YYYY-MM-DD`, digits in the right places.
pub fn is_iso_date(s: &str) -> bool {
    s.len() == 10
        && s.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Today plus `days` on the local day, as an ISO date.
pub fn date_after(days: i64) -> String {
    day_plus(chrono::Local::now().date_naive(), days)
}

/// `day` plus `days`, as an ISO date.
fn day_plus(day: chrono::NaiveDate, days: i64) -> String {
    (day + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

/// Whether a follow-up date has passed: it is before `today`, so today itself is not late.
fn overdue(follow_up_by: &str, today: &str) -> bool {
    !follow_up_by.is_empty() && follow_up_by < today
}

/// Whether a decision is waiting on its follow-up by `horizon` (an ISO date).
fn is_due(decision: &DecisionSummary, horizon: &str) -> bool {
    matches!(decision.status.as_str(), "decided" | "revised")
        && !decision.follow_up_by.is_empty()
        && decision.follow_up_by.as_str() <= horizon
}

/// The day of the last `### Follow-up YYYY-MM-DD` heading, for a page followed up before
/// `followed_up` was recorded; empty when there is none.
fn last_follow_up(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| line.strip_prefix("### Follow-up "))
        .filter_map(|rest| rest.get(..10))
        .rfind(|day| is_iso_date(day))
        .unwrap_or_default()
        .to_string()
}

fn property_string(props: &[(String, serde_json::Value)], key: &str) -> String {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string().trim_matches('"').to_string(),
        })
        .unwrap_or_default()
}

fn property_list(props: &[(String, serde_json::Value)], key: &str) -> Vec<String> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| match v {
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_string))
                .collect(),
            serde_json::Value::String(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

impl BrainManager {
    /// Consult the brain: ranked pages, the decisions touching the question with their
    /// status, the follow-ups due, and a consultation id recorded with the hits.
    pub fn ask(
        &self,
        question: &str,
        limit: Option<usize>,
        embedder: Option<&dyn Embedder>,
    ) -> Result<Consultation, String> {
        let question = question.trim();
        if question.is_empty() {
            return Err("ask needs a question".to_string());
        }
        let limit = limit.unwrap_or(8).max(1);
        let hits = match embedder {
            Some(e) => self.search_hybrid(question, Some(limit + 5), None, e)?,
            None => self.search(question, Some(limit + 5), None)?,
        };
        let pages: Vec<BrainSearchResult> = hits
            .into_iter()
            .filter(|p| p.page_type != DECISION_TYPE)
            .take(limit)
            .collect();
        let decisions: Vec<DecisionSummary> = self
            .search(question, Some(10), Some(DECISION_TYPE))?
            .into_iter()
            .filter_map(|r| self.decision_summary(&r.slug).ok().flatten())
            .collect();
        let due = self.due(0)?.due;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let slugs: Vec<&str> = pages.iter().map(|p| p.slug.as_str()).collect();
        let hits_json = serde_json::to_string(&slugs).map_err(|e| e.to_string())?;
        {
            let conn = self.db.conn()?;
            conn.execute(
                "INSERT INTO brain_consultations (id, question, hits, created_at, outcome) \
                 VALUES (?1, ?2, ?3, ?4, NULL)",
                rusqlite::params![id, question, hits_json, super::unix_now()],
            )
            .map_err(|e| format!("record the consultation: {e}"))?;
        }
        Ok(Consultation {
            id,
            question: question.to_string(),
            pages,
            decisions,
            due,
        })
    }

    /// The question and the hits of a consultation.
    fn consultation(&self, id: &str) -> Result<(String, Vec<String>), String> {
        let conn = self.db.conn()?;
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT question, hits FROM brain_consultations WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let (question, hits) =
            row.ok_or_else(|| format!("no consultation {id}; call ask first"))?;
        let hits: Vec<String> = serde_json::from_str(&hits).unwrap_or_default();
        Ok((question, hits))
    }

    fn set_consultation_outcome(&self, id: &str, outcome: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        let n = conn
            .execute(
                "UPDATE brain_consultations SET outcome = ?1 WHERE id = ?2",
                rusqlite::params![outcome, id],
            )
            .map_err(|e| format!("record the outcome: {e}"))?;
        if n == 0 {
            return Err(format!("no consultation {id}; call ask first"));
        }
        Ok(())
    }

    /// Write the decision page, link it to every consulted page, and mark the
    /// consultation's outcome.
    pub fn decide(&self, input: &Decide) -> Result<BrainPage, String> {
        for (name, value) in [
            ("title", &input.title),
            ("choice", &input.choice),
            ("rationale", &input.rationale),
        ] {
            if value.trim().is_empty() {
                return Err(format!("decide needs a {name}"));
            }
        }
        if let Some(date) = &input.follow_up_by {
            if !is_iso_date(date) {
                return Err(format!("follow_up_by is not a date (YYYY-MM-DD): {date}"));
            }
        }
        let (question, hits) = self.consultation(&input.consultation)?;
        if let Some(old) = &input.supersedes {
            let page = self
                .read_page(old)?
                .ok_or_else(|| format!("no page to supersede: {old}"))?;
            if page.page_type != DECISION_TYPE {
                return Err(format!("{old} is not a decision"));
            }
        }
        let today = today_iso();
        let mut body = format!(
            "## Question\n\n{question}\n\n## Choice\n\n{}\n\n## Rationale\n\n{}\n\n## Alternatives\n\n",
            input.choice.trim(),
            input.rationale.trim()
        );
        if input.alternatives.is_empty() {
            body.push_str("None recorded.\n");
        } else {
            for alt in &input.alternatives {
                body.push_str(&format!("- {}\n", alt.trim()));
            }
        }
        body.push_str("\n## Consulted\n\n");
        if hits.is_empty() {
            body.push_str("Nothing in the vault matched the question.\n");
        } else {
            for slug in &hits {
                body.push_str(&format!("- [[{slug}]]\n"));
            }
        }
        if let Some(old) = &input.supersedes {
            body.push_str(&format!("\nSupersedes [[{old}]].\n"));
        }
        let created = self.create_page(DECISION_TYPE, input.title.trim(), &body)?;
        let mut fm = created.frontmatter.clone();
        fm.extra
            .insert("question".to_string(), serde_json::json!(question));
        fm.extra
            .insert("status".to_string(), serde_json::json!("decided"));
        fm.extra
            .insert("decided".to_string(), serde_json::json!(today));
        fm.extra.insert(
            "consultation".to_string(),
            serde_json::json!(input.consultation),
        );
        fm.extra
            .insert("consulted".to_string(), serde_json::json!(hits));
        if let Some(date) = &input.follow_up_by {
            fm.extra
                .insert("follow_up_by".to_string(), serde_json::json!(date));
        }
        if let Some(old) = &input.supersedes {
            fm.extra
                .insert("supersedes".to_string(), serde_json::json!(old));
        }
        let raw = render_page(&fm, &body, "")?;
        let page = self.write_raw(&created.slug, &raw)?;
        for slug in &hits {
            let _ = self.add_timeline(
                slug,
                &today,
                SOURCE,
                &format!("Decision: {} ([[{}]])", input.title.trim(), page.slug),
                Some(input.choice.trim()),
            );
        }
        if let Some(old) = &input.supersedes {
            self.set_property(old, "status", serde_json::json!("superseded"))?;
            self.set_property(old, "superseded_by", serde_json::json!(page.slug))?;
            let _ = self.add_timeline(
                old,
                &today,
                SOURCE,
                &format!("Superseded by [[{}]]", page.slug),
                None,
            );
        }
        self.set_consultation_outcome(&input.consultation, &page.slug)?;
        Ok(page)
    }

    /// Append the outcome, set the status, clear or reschedule the date.
    pub fn follow_up(&self, input: &FollowUp) -> Result<BrainPage, String> {
        if !FOLLOW_UP_STATUSES.contains(&input.status.as_str()) {
            return Err(format!(
                "status must be one of {}",
                FOLLOW_UP_STATUSES.join(", ")
            ));
        }
        if input.outcome.trim().is_empty() {
            return Err("follow_up needs an outcome".to_string());
        }
        if let Some(date) = &input.follow_up_by {
            if !is_iso_date(date) {
                return Err(format!("follow_up_by is not a date (YYYY-MM-DD): {date}"));
            }
        }
        let page = self
            .read_page(&input.slug)?
            .ok_or_else(|| format!("no decision {}", input.slug))?;
        if page.page_type != DECISION_TYPE {
            return Err(format!("{} is not a decision", input.slug));
        }
        let successor = match (input.status.as_str(), &input.successor) {
            ("superseded", None) => return Err("superseded needs the successor's slug".to_string()),
            ("superseded", Some(s)) => {
                let next = self
                    .read_page(s)?
                    .ok_or_else(|| format!("no successor {s}"))?;
                if next.page_type != DECISION_TYPE {
                    return Err(format!("{s} is not a decision"));
                }
                Some(s.clone())
            }
            _ => None,
        };
        let today = today_iso();
        let raw = self
            .vault
            .read_page(&input.slug)?
            .ok_or_else(|| format!("no decision {}", input.slug))?;
        let entry = format!(
            "\n### Follow-up {today}: {}\n\n{}\n",
            input.status,
            input.outcome.trim()
        );
        let raw = match raw.find("\n## Timeline") {
            Some(i) => format!("{}{}{}", &raw[..i], entry, &raw[i..]),
            None => format!("{}\n{entry}", raw.trim_end_matches('\n')),
        };
        self.write_raw(&input.slug, &raw)?;
        self.set_property(&input.slug, "status", serde_json::json!(input.status))?;
        self.set_property(&input.slug, "followed_up", serde_json::json!(today))?;
        match &input.follow_up_by {
            Some(date) => {
                self.set_property(&input.slug, "follow_up_by", serde_json::json!(date))?;
            }
            None => {
                let _ = self.remove_property(&input.slug, "follow_up_by");
            }
        }
        if let Some(s) = &successor {
            self.set_property(&input.slug, "superseded_by", serde_json::json!(s))?;
        }
        let _ = self.add_timeline(
            &input.slug,
            &today,
            SOURCE,
            &format!("Follow-up: {}", input.status),
            Some(input.outcome.trim()),
        );
        self.read_page(&input.slug)?
            .ok_or_else(|| format!("no decision {}", input.slug))
    }

    /// Record that a consultation led to no decision.
    pub fn no_decision(&self, consultation: &str, reason: &str) -> Result<(), String> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("no_decision needs a reason".to_string());
        }
        self.set_consultation_outcome(consultation, &format!("no decision: {reason}"))
    }

    /// One decision's summary, or `None` when the slug is not a decision.
    pub fn decision_summary(&self, slug: &str) -> Result<Option<DecisionSummary>, String> {
        let Some(page) = self.read_page(slug)? else {
            return Ok(None);
        };
        if page.page_type != DECISION_TYPE {
            return Ok(None);
        }
        let raw = self.vault.read_page(slug)?.unwrap_or_default();
        let props = properties_of(&raw);
        let status = {
            let s = property_string(&props, "status");
            if s.is_empty() {
                "decided".to_string()
            } else {
                s
            }
        };
        let follow_up_by = property_string(&props, "follow_up_by");
        let today = today_iso();
        Ok(Some(DecisionSummary {
            slug: slug.to_string(),
            title: page.title,
            question: property_string(&props, "question"),
            status,
            decided: property_string(&props, "decided"),
            overdue: overdue(&follow_up_by, &today),
            follow_up_by,
            followed_up: {
                let recorded = property_string(&props, "followed_up");
                if recorded.is_empty() {
                    last_follow_up(&raw)
                } else {
                    recorded
                }
            },
            superseded_by: property_string(&props, "superseded_by"),
        }))
    }

    /// The follow-ups due within `days` (today and overdue at zero) and every decision,
    /// newest first.
    pub fn due(&self, days: i64) -> Result<Due, String> {
        let mut all: Vec<DecisionSummary> = self
            .list_pages(Some(DECISION_TYPE), Some(1000))?
            .iter()
            .filter_map(|s| self.decision_summary(&s.slug).ok().flatten())
            .collect();
        all.sort_by(|a, b| b.decided.cmp(&a.decided).then_with(|| a.slug.cmp(&b.slug)));
        let horizon = date_after(days);
        let mut due: Vec<DecisionSummary> = all
            .iter()
            .filter(|d| is_due(d, &horizon))
            .cloned()
            .collect();
        due.sort_by(|a, b| a.follow_up_by.cmp(&b.follow_up_by));
        Ok(Due { due, all })
    }

    /// The typed edges of every decision page: `consulted`, `supersedes`, `follows_up`
    /// (from the superseded page to its successor).
    pub(super) fn decision_edges(&self) -> Result<Vec<(String, String, String)>, String> {
        let mut edges = Vec::new();
        for summary in self.list_pages(Some(DECISION_TYPE), Some(1000))? {
            let Some(raw) = self.vault.read_page(&summary.slug)? else {
                continue;
            };
            let props = properties_of(&raw);
            for target in property_list(&props, "consulted") {
                edges.push((summary.slug.clone(), target, "consulted".to_string()));
            }
            let old = property_string(&props, "supersedes");
            if !old.is_empty() {
                edges.push((summary.slug.clone(), old, "supersedes".to_string()));
            }
            let next = property_string(&props, "superseded_by");
            if !next.is_empty() {
                edges.push((summary.slug.clone(), next, "follows_up".to_string()));
            }
        }
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn due_uses_the_local_day_at_night() {
        use super::super::frontmatter::day_of;
        use chrono::TimeZone;
        // 23:30 on 2 October at UTC−5, when the UTC day is already the 3rd.
        let utc = chrono::Utc.with_ymd_and_hms(2026, 10, 3, 4, 30, 0).unwrap();
        let local = utc.with_timezone(&chrono::FixedOffset::west_opt(5 * 3600).unwrap());
        let today = day_of(&local);
        assert_eq!(today, "2026-10-02");
        let horizon = super::day_plus(local.date_naive(), 0);
        assert_eq!(horizon, today);
        assert!(!super::overdue("2026-10-02", &today));
        assert!(super::overdue("2026-10-01", &today));
        assert!(!super::overdue("", &today));
        let summary = |follow_up_by: &str| super::DecisionSummary {
            slug: "decisions/x".into(),
            title: "X".into(),
            question: String::new(),
            status: "decided".into(),
            decided: "2026-10-01".into(),
            follow_up_by: follow_up_by.into(),
            overdue: false,
            followed_up: String::new(),
            superseded_by: String::new(),
        };
        assert!(super::is_due(&summary("2026-10-02"), &horizon));
        assert!(!super::is_due(&summary("2026-10-03"), &horizon));
    }

    fn brain(name: &str) -> (PathBuf, BrainManager) {
        let dir = std::env::temp_dir().join(format!("rusty_loop_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        let bm = BrainManager::new(Arc::new(db), dir.clone());
        bm.ensure_vault().unwrap();
        bm.create_page(
            "project",
            "Orbit",
            "Orbit keeps its index in SQLite, rebuilt from the vault.\n",
        )
        .unwrap();
        (dir, bm)
    }

    fn decide(bm: &BrainManager, title: &str, follow_up_by: Option<&str>) -> BrainPage {
        let c = bm.ask("SQLite index", None, None).unwrap();
        bm.decide(&Decide {
            consultation: c.id,
            title: title.to_string(),
            choice: "Keep SQLite as the index".to_string(),
            rationale: "It is rebuildable from the vault.".to_string(),
            alternatives: vec!["Postgres".to_string()],
            follow_up_by: follow_up_by.map(str::to_string),
            supersedes: None,
        })
        .unwrap()
    }

    #[test]
    fn ask_records_a_consultation_and_ranks_pages() {
        let (dir, bm) = brain("ask");
        let c = bm.ask("SQLite index", None, None).unwrap();
        assert_eq!(c.id.len(), 32);
        assert!(
            c.pages.iter().any(|p| p.slug == "projects/orbit"),
            "{:?}",
            c.pages
        );
        assert!(c.decisions.is_empty() && c.due.is_empty());
        let n: i64 = bm
            .db
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM brain_consultations WHERE id = ?1 AND outcome IS NULL",
                [&c.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert!(bm.ask("   ", None, None).is_err());
        bm.no_decision(&c.id, "the question answered itself")
            .unwrap();
        assert!(bm.no_decision("nope", "x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decide_writes_a_linked_decision_page_and_timeline_entries() {
        let (dir, bm) = brain("decide");
        let page = decide(&bm, "Keep SQLite", Some(&date_after(7)));
        assert!(page.slug.starts_with("decisions/"), "{}", page.slug);
        assert_eq!(page.page_type, "decision");
        let raw = bm.vault.read_page(&page.slug).unwrap().unwrap();
        assert!(raw.contains("status: decided"), "{raw}");
        assert!(raw.contains("[[projects/orbit]]"), "{raw}");
        assert!(raw.contains("## Alternatives") && raw.contains("- Postgres"));
        let summary = bm.decision_summary(&page.slug).unwrap().unwrap();
        assert_eq!(summary.status, "decided");
        assert_eq!(summary.follow_up_by, date_after(7));
        assert_eq!(summary.question, "SQLite index");
        let orbit = bm.read_page("projects/orbit").unwrap().unwrap();
        assert!(orbit.timeline.contains("Keep SQLite"), "{}", orbit.timeline);
        let outcome: String = bm
            .db
            .conn()
            .unwrap()
            .query_row("SELECT outcome FROM brain_consultations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(outcome, page.slug);
        assert!(bm
            .decide(&Decide {
                consultation: "nope".into(),
                title: "x".into(),
                choice: "y".into(),
                rationale: "z".into(),
                ..Default::default()
            })
            .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn follow_up_records_the_date_and_the_successor() {
        let (dir, bm) = brain("follow_up_fields");
        let first = decide(&bm, "Keep SQLite", Some(&date_after(1)));
        let fresh = bm.decision_summary(&first.slug).unwrap().unwrap();
        assert_eq!(
            (fresh.followed_up.as_str(), fresh.superseded_by.as_str()),
            ("", "")
        );
        let second = decide(&bm, "Move to Postgres", None);
        bm.follow_up(&FollowUp {
            slug: first.slug.clone(),
            outcome: "Outgrown.".into(),
            status: "superseded".into(),
            successor: Some(second.slug.clone()),
            ..Default::default()
        })
        .unwrap();
        let raw = bm.vault.read_page(&first.slug).unwrap().unwrap();
        let today = today_iso();
        assert!(raw.contains(&format!("followed_up: {today}")), "{raw}");
        let summary = bm.decision_summary(&first.slug).unwrap().unwrap();
        assert_eq!(summary.followed_up, today);
        assert_eq!(summary.superseded_by, second.slug);
        let in_due = bm.due(0).unwrap();
        let listed = in_due.all.iter().find(|d| d.slug == first.slug).unwrap();
        assert_eq!(listed.superseded_by, second.slug);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_older_follow_up_is_read_from_its_heading() {
        let raw = "---\ntype: decision\n---\n\n## Choice\n\nX\n\n### Follow-up 2026-09-10: kept\n\nFine.\n\n### Follow-up 2026-09-24: revised\n\nChanged.\n\n### Follow-up later: bad\n";
        assert_eq!(last_follow_up(raw), "2026-09-24");
        assert_eq!(last_follow_up("no follow-ups\n"), "");
    }

    #[test]
    fn follow_up_sets_the_status_and_the_date() {
        let (dir, bm) = brain("follow_up");
        let first = decide(&bm, "Keep SQLite", Some(&date_after(1)));
        let kept = bm
            .follow_up(&FollowUp {
                slug: first.slug.clone(),
                outcome: "Still fine.".into(),
                status: "kept".into(),
                ..Default::default()
            })
            .unwrap();
        let summary = bm.decision_summary(&kept.slug).unwrap().unwrap();
        assert_eq!(
            (summary.status.as_str(), summary.follow_up_by.as_str()),
            ("kept", "")
        );
        assert!(
            kept.compiled_truth.contains("### Follow-up")
                && kept.compiled_truth.contains("Still fine."),
            "{}",
            kept.compiled_truth
        );
        assert!(kept.timeline.contains("Follow-up: kept"));
        let second = decide(&bm, "Move to Postgres", None);
        let old = bm
            .follow_up(&FollowUp {
                slug: first.slug.clone(),
                outcome: "Outgrown.".into(),
                status: "superseded".into(),
                successor: Some(second.slug.clone()),
                ..Default::default()
            })
            .unwrap();
        let summary = bm.decision_summary(&old.slug).unwrap().unwrap();
        assert_eq!(summary.status, "superseded");
        assert!(
            bm.follow_up(&FollowUp {
                slug: first.slug.clone(),
                outcome: "x".into(),
                status: "superseded".into(),
                ..Default::default()
            })
            .is_err(),
            "a successor is needed"
        );
        assert!(
            bm.follow_up(&FollowUp {
                slug: "projects/orbit".into(),
                outcome: "x".into(),
                status: "kept".into(),
                ..Default::default()
            })
            .is_err(),
            "not a decision"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn due_lists_open_follow_ups() {
        let (dir, bm) = brain("due");
        let late = decide(&bm, "Late one", Some(&date_after(-1)));
        let soon = decide(&bm, "Soon one", Some(&date_after(20)));
        let _none = decide(&bm, "No date", None);
        let today = bm.due(0).unwrap();
        assert_eq!(today.all.len(), 3);
        assert_eq!(
            today
                .due
                .iter()
                .map(|d| d.slug.as_str())
                .collect::<Vec<_>>(),
            [late.slug.as_str()]
        );
        assert!(today.due[0].overdue);
        let month = bm.due(30).unwrap();
        assert_eq!(month.due.len(), 2);
        assert_eq!(month.due[1].slug, soon.slug);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn graph_carries_typed_edges_for_decisions() {
        let (dir, bm) = brain("graph");
        let first = decide(&bm, "Keep SQLite", None);
        let c = bm.ask("SQLite index", None, None).unwrap();
        let second = bm
            .decide(&Decide {
                consultation: c.id,
                title: "Move on".into(),
                choice: "Postgres".into(),
                rationale: "Scale.".into(),
                supersedes: Some(first.slug.clone()),
                ..Default::default()
            })
            .unwrap();
        let graph = bm.graph(&super::super::GraphOptions::default()).unwrap();
        let kinds: Vec<(String, String, String)> = graph
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone(), e.kind.clone()))
            .collect();
        assert!(
            kinds.contains(&(
                first.slug.clone(),
                "projects/orbit".to_string(),
                "consulted".to_string()
            )),
            "{kinds:?}"
        );
        assert!(
            kinds.contains(&(
                second.slug.clone(),
                first.slug.clone(),
                "supersedes".to_string()
            )),
            "{kinds:?}"
        );
        assert!(
            kinds.contains(&(
                first.slug.clone(),
                second.slug.clone(),
                "follows_up".to_string()
            )),
            "{kinds:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
