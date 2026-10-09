//! Entity extraction from conversation text: a heuristic that finds capitalized
//! multi-word names (people, companies, projects) at no cost and with no model call.
//! `conversation_archive` uses it to link archived conversations to brain pages.

/// An entity detected from conversation text.
#[derive(Debug, Clone)]
pub struct DetectedEntity {
    /// Entity name as it appeared in text.
    pub name: String,
    /// Inferred type: "person", "company", or "project".
    pub entity_type: String,
}

/// Extract entities using passive heuristic (capitalized multi-word names).
///
/// Finds sequences of 2+ consecutive capitalized words, filters common
/// false positives, and infers entity type from suffixes.
pub fn extract_entities_passive(text: &str) -> Vec<DetectedEntity> {
    let mut entities: Vec<DetectedEntity> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let mut i = 0;

        while i < words.len() {
            // Look for sequences of capitalized words
            if is_capitalized_word(words[i]) && !is_stop_word(words[i]) {
                let start = i;
                i += 1;
                while i < words.len() && is_capitalized_word(words[i]) {
                    i += 1;
                }

                // Only keep sequences of 2+ words (single caps words are too noisy)
                if i - start >= 2 {
                    let name_words: Vec<&str> = words[start..i]
                        .iter()
                        .map(|w| strip_punctuation(w))
                        .collect();
                    let name = name_words.join(" ");

                    // Skip if too short or a known false positive phrase
                    if name.len() >= 4 && !is_false_positive_phrase(&name) {
                        let lower = name.to_lowercase();
                        if !seen.contains(&lower) {
                            seen.push(lower);
                            let entity_type = infer_entity_type(&name);
                            entities.push(DetectedEntity { name, entity_type });
                        }
                    }
                }
            } else {
                i += 1;
            }
        }
    }

    entities
}

/// Check if a word starts with an uppercase letter.
fn is_capitalized_word(word: &str) -> bool {
    let clean = strip_punctuation(word);
    clean
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

/// Strip leading/trailing punctuation from a word.
fn strip_punctuation(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Check if a word is a common stop word (frequent false positives).
fn is_stop_word(word: &str) -> bool {
    let clean = strip_punctuation(word).to_lowercase();
    matches!(
        clean.as_str(),
        "i" | "the"
            | "this"
            | "that"
            | "these"
            | "those"
            | "here"
            | "there"
            | "it"
            | "its"
            | "my"
            | "we"
            | "our"
            | "you"
            | "your"
            | "he"
            | "she"
            | "they"
            | "if"
            | "so"
            | "but"
            | "or"
            | "and"
            | "for"
            | "not"
            | "no"
            | "yes"
            | "ok"
            | "let"
            | "can"
            | "will"
            | "would"
            | "should"
            | "could"
            | "may"
            | "just"
            | "also"
            | "sure"
            | "thank"
            | "thanks"
            | "please"
            | "note"
            | "however"
            | "therefore"
            | "furthermore"
            | "additionally"
            | "later"
            | "after"
            | "before"
            | "then"
            | "next"
            | "first"
            | "finally"
            | "recently"
            | "today"
            | "yesterday"
            | "tomorrow"
            | "when"
            | "where"
            | "while"
            | "since"
            | "until"
            | "during"
            | "about"
            | "each"
            | "every"
            | "both"
            | "all"
            | "any"
            | "some"
            | "most"
            | "many"
            | "much"
            | "more"
            | "other"
            | "new"
            | "old"
    )
}

/// Check if a multi-word name is a common false positive phrase.
fn is_false_positive_phrase(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Common sentence starters and filler phrases
    let phrases = [
        "let me",
        "here is",
        "here are",
        "thank you",
        "phase plan",
        "phase design",
        "phase implement",
        "for example",
        "in order",
        "on the",
        "at the",
        "key context",
        "compiled truth",
    ];
    for phrase in &phrases {
        if lower == *phrase {
            return true;
        }
    }

    // Month + day combinations
    let months = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let first_word = lower.split_whitespace().next().unwrap_or("");
    if months.contains(&first_word) {
        return true;
    }

    false
}

/// Infer entity type from the name.
///
/// Company suffixes (Corp, Inc, LLC, Ltd) → "company".
/// Default → "person" (most entities in conversation are people).
fn infer_entity_type(name: &str) -> String {
    let lower = name.to_lowercase();
    let company_suffixes = [
        "corp", "inc", "llc", "ltd", "co", "company", "group", "labs",
    ];

    for suffix in &company_suffixes {
        if lower.ends_with(suffix) {
            return "company".to_string();
        }
    }

    "person".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_extracts_names() {
        let text = "I talked to Sarah Chen about the project yesterday.";
        let entities = extract_entities_passive(text);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Sarah Chen");
        assert_eq!(entities[0].entity_type, "person");
    }

    #[test]
    fn passive_extracts_companies() {
        let text = "Working with Acme Corp on the integration.";
        let entities = extract_entities_passive(text);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Acme Corp");
        assert_eq!(entities[0].entity_type, "company");
    }

    #[test]
    fn passive_filters_false_positives() {
        let text = "Thank You. Let Me check. Here Is the result.";
        let entities = extract_entities_passive(text);
        assert!(entities.is_empty());
    }

    #[test]
    fn passive_deduplicates() {
        let text = "Sarah Chen said hello. Later, Sarah Chen also mentioned it.";
        let entities = extract_entities_passive(text);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn passive_mixed_entities() {
        let text = "Sarah Chen at Acme Corp discussed the integration with Bob Williams.";
        let entities = extract_entities_passive(text);
        let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Sarah Chen"));
        assert!(names.contains(&"Acme Corp"));
        assert!(names.contains(&"Bob Williams"));
        assert_eq!(entities.len(), 3);
    }

    #[test]
    fn passive_skips_single_capitalized_words() {
        let text = "Rust is great. Python too.";
        let entities = extract_entities_passive(text);
        assert!(entities.is_empty());
    }

    #[test]
    fn infer_type_company_suffixes() {
        assert_eq!(infer_entity_type("Acme Corp"), "company");
        assert_eq!(infer_entity_type("Tech Labs"), "company");
        assert_eq!(infer_entity_type("OpenAI Inc"), "company");
        assert_eq!(infer_entity_type("Sarah Chen"), "person");
    }
}
