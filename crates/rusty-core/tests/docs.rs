//! The configuration reference names every setting the code reads, so a new setting
//! cannot ship undocumented.

use rusty_core::brain::semantic::{SETTING_MODEL, SETTING_OLLAMA_URL, SETTING_PROVIDER};
use rusty_core::engine::pin_lock::TIMEOUT_SETTING;
use rusty_core::skills::{SETTING_ENABLED, SETTING_PATH};

#[test]
fn the_configuration_reference_names_every_setting() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/configuration.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    for key in [
        SETTING_PROVIDER,
        SETTING_MODEL,
        SETTING_OLLAMA_URL,
        TIMEOUT_SETTING,
        SETTING_ENABLED,
        SETTING_PATH,
        "brain_vault_path",
        "notes_path",
    ] {
        assert!(
            doc.contains(&format!("| `{key}` |")),
            "docs/configuration.md has no row for the setting `{key}`"
        );
    }
}
