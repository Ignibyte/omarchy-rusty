//! A line diff for the agent's edits: what an `Edit` replaces, as rows of kept, added
//! and removed lines, for a card to draw. Common prefix and suffix lines are matched
//! first; the middle is a longest common subsequence, capped so a huge rewrite answers
//! quickly as a removal followed by an addition.

/// One row of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Same(String),
    Add(String),
    Del(String),
}

/// The most cells the LCS table may hold before the middle is answered wholesale.
const CELL_CAP: usize = 2_000_000;

/// The rows between `old` and `new`, split on newlines; in a changed block the removed
/// lines come before the added ones.
pub fn diff_lines(old: &str, new: &str) -> Vec<Row> {
    let a: Vec<&str> = split(old);
    let b: Vec<&str> = split(new);
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut rows: Vec<Row> = a[..prefix]
        .iter()
        .map(|l| Row::Same(l.to_string()))
        .collect();
    let mid_a = &a[prefix..a.len() - suffix];
    let mid_b = &b[prefix..b.len() - suffix];
    rows.extend(middle(mid_a, mid_b));
    rows.extend(
        a[a.len() - suffix..]
            .iter()
            .map(|l| Row::Same(l.to_string())),
    );
    rows
}

fn split(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n').collect()
}

fn middle(a: &[&str], b: &[&str]) -> Vec<Row> {
    if a.is_empty() {
        return b.iter().map(|l| Row::Add(l.to_string())).collect();
    }
    if b.is_empty() {
        return a.iter().map(|l| Row::Del(l.to_string())).collect();
    }
    if a.len().saturating_mul(b.len()) > CELL_CAP {
        let mut rows: Vec<Row> = a.iter().map(|l| Row::Del(l.to_string())).collect();
        rows.extend(b.iter().map(|l| Row::Add(l.to_string())));
        return rows;
    }
    // lcs[i][j] is the length of the longest common subsequence of a[i..] and b[j..].
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut rows = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            rows.push(Row::Same(a[i].to_string()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            rows.push(Row::Del(a[i].to_string()));
            i += 1;
        } else {
            rows.push(Row::Add(b[j].to_string()));
            j += 1;
        }
    }
    rows.extend(a[i..].iter().map(|l| Row::Del(l.to_string())));
    rows.extend(b[j..].iter().map(|l| Row::Add(l.to_string())));
    rows
}

/// The rows as JSON: `[{"kind":"same"|"add"|"del","text":"…"}]`.
pub fn to_json(rows: &[Row]) -> String {
    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let (kind, text) = match row {
                Row::Same(t) => ("same", t),
                Row::Add(t) => ("add", t),
                Row::Del(t) => ("del", t),
            };
            serde_json::json!({ "kind": kind, "text": text })
        })
        .collect();
    serde_json::Value::Array(items).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| match r {
                Row::Same(_) => '=',
                Row::Add(_) => '+',
                Row::Del(_) => '-',
            })
            .collect()
    }

    #[test]
    fn identical_text_is_all_kept() {
        let rows = diff_lines("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(kinds(&rows), "===");
        assert_eq!(rows[0], Row::Same("a".into()));
    }

    #[test]
    fn insertions_deletions_and_a_changed_block_keep_their_order() {
        assert_eq!(kinds(&diff_lines("a\nc", "a\nb\nc")), "=+=");
        assert_eq!(kinds(&diff_lines("a\nb\nc", "a\nc")), "=-=");
        let rows = diff_lines("one\ntwo\nthree\nfour", "one\n2\n3\nfour");
        assert_eq!(kinds(&rows), "=--++=", "{rows:?}");
        assert_eq!(rows[1], Row::Del("two".into()));
        assert_eq!(rows[3], Row::Add("2".into()));
    }

    #[test]
    fn a_write_is_all_added_and_an_erasure_all_removed() {
        assert_eq!(kinds(&diff_lines("", "x\ny")), "++");
        assert_eq!(kinds(&diff_lines("x\ny\n", "")), "--");
        assert!(diff_lines("", "").is_empty());
    }

    #[test]
    fn trailing_newlines_and_unicode_lines_are_plain_lines() {
        assert_eq!(kinds(&diff_lines("a\nb", "a\nb\n")), "==");
        let rows = diff_lines("héllo\nwörld", "héllo\nwelt");
        assert_eq!(kinds(&rows), "=-+");
        assert_eq!(rows[2], Row::Add("welt".into()));
    }

    #[test]
    fn a_huge_rewrite_answers_wholesale() {
        let old: String = (0..1500).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..1500).map(|i| format!("new {i}\n")).collect();
        let rows = diff_lines(&old, &new);
        assert_eq!(rows.len(), 3000);
        assert!(rows[..1500].iter().all(|r| matches!(r, Row::Del(_))));
        assert!(rows[1500..].iter().all(|r| matches!(r, Row::Add(_))));
    }

    #[test]
    fn json_rows_carry_kind_and_text() {
        let json = to_json(&diff_lines("a", "b"));
        let rows: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(rows[0]["kind"], "del");
        assert_eq!(rows[0]["text"], "a");
        assert_eq!(rows[1]["kind"], "add");
        assert_eq!(rows[1]["text"], "b");
    }
}
