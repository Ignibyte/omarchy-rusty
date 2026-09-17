//! Build script: generates the cxx-qt bridges, runs moc on the C++ highlighter, and
//! bundles the QML module `dev.ignibyte.rusty` (the workspace and its views) into the
//! binary.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let manifest =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("dev.ignibyte.rusty")
            .version(1, 0)
            .qml_files([
                "qml/Main.qml",
                "qml/Icon.qml",
                "qml/Explorer.qml",
                "qml/FileTab.qml",
                "qml/DecisionsPage.qml",
                "qml/SearchPane.qml",
                "qml/NoteTab.qml",
                "qml/RightPane.qml",
                "qml/GraphView.qml",
                "qml/BookmarksPane.qml",
                "qml/TopBar.qml",
                "qml/Scanlines.qml",
                "qml/AgentTerminal.qml",
                "qml/QuickSwitcher.qml",
                "qml/CommandPalette.qml",
                "qml/TasksPage.qml",
                "qml/MemoryPage.qml",
                "qml/SkillsPage.qml",
                "qml/SecretsPage.qml",
                "qml/SettingsPage.qml",
                "qml/Splitter.qml",
            ]),
    )
    .files([
        "src/theme.rs",
        "src/desk.rs",
        "src/folders.rs",
        "src/terminals.rs",
        "src/backend.rs",
        "src/assistant.rs",
        "src/agents.rs",
        "src/markdown.rs",
    ])
    .cpp_files([
        manifest.join("cpp/highlighter.h"),
        manifest.join("cpp/highlighter.cpp"),
        manifest.join("cpp/tools.h"),
        manifest.join("cpp/tools.cpp"),
    ])
    .include_dir(manifest.join("cpp"))
    .qt_module("Quick")
    .qt_module("Qml");
    // SAFETY: the closure only adds diagnostic flags. It touches no include paths,
    // defines or sources the generated code depends on. GCC 16 warns about Qt's own
    // headers (-Wsfinae-incomplete) and about cxx's generated Vec constructor
    // (-Wmaybe-uninitialized), neither of which is ours to fix.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("-Wno-sfinae-incomplete");
            cc.flag_if_supported("-Wno-maybe-uninitialized");
        })
    };
    builder.build();
    println!("cargo:rerun-if-changed=cpp/highlighter.h");
    println!("cargo:rerun-if-changed=cpp/highlighter.cpp");
    println!("cargo:rerun-if-changed=cpp/tools.h");
    println!("cargo:rerun-if-changed=cpp/tools.cpp");
}
