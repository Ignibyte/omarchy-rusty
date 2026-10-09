//! Drives the built `rusty-mcp` binary over stdio with the rmcp client, in a scratch
//! HOME, the way Claude Code or Codex would. Runs in CI.

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;
use tokio::process::Command;

fn scratch_home() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("rusty-mcp-smoke-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect()
}

fn args(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().cloned().expect("a JSON object")
}

#[tokio::test]
async fn a_real_client_can_list_call_and_read() {
    let home = scratch_home();
    // A store script beside its skill, for the script tools.
    let skill_dir = home.join(".rusty/skills/.claude/skills/dev-box-usb");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: dev-box-usb\ndescription: Reset the USB controller.\n---\n\nRun `rusty usb-reset`.\n",
    )
    .unwrap();
    std::fs::write(
        skill_dir.join("usb-reset.sh"),
        "#!/usr/bin/env bash\necho \"reset $1\"\n",
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-mcp"));
    cmd.env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home.join("run"));
    let client =
        ().serve(TokioChildProcess::new(cmd).expect("spawn rusty-mcp"))
            .await
            .expect("initialize");

    let info = client.peer_info().expect("server info");
    let server_name = info
        .server_info
        .as_ref()
        .map(|i| i.name.to_string())
        .unwrap_or_default();
    assert!(server_name.contains("rusty"), "{server_name}");

    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.len() >= 65, "{} tools", tools.len());
    for name in [
        "list_task_groups",
        "brain_capture",
        "settings_list",
        "brain_tree",
        "brain_render",
        "brain_rename",
    ] {
        assert!(tools.iter().any(|t| t.name == name), "missing {name}");
    }

    let created = client
        .call_tool(
            CallToolRequestParams::new("create_task_group")
                .with_arguments(args(serde_json::json!({"name": "Smoke"}))),
        )
        .await
        .unwrap();
    assert!(!created.is_error.unwrap_or(false), "{}", text_of(&created));
    let groups = client
        .call_tool(CallToolRequestParams::new("list_task_groups"))
        .await
        .unwrap();
    assert!(text_of(&groups).contains("Smoke"), "{}", text_of(&groups));

    let resources = client.list_all_resources().await.unwrap();
    assert!(resources.iter().any(|r| r.uri == "rusty://tasks"));

    // The workspace path: a page in a folder, rendered, edited whole, moved with its
    // links rewritten, and the tree that shows it.
    let created = client
        .call_tool(
            CallToolRequestParams::new("brain_new_page").with_arguments(args(
                serde_json::json!({"folder": "projects", "name": "Smoke plan"}),
            )),
        )
        .await
        .unwrap();
    assert_eq!(
        text_of(&created),
        "\"projects/Smoke plan\"",
        "{}",
        text_of(&created)
    );
    // The change cursor: a write after it is returned, with the cursor to pass next
    // (TICKET-035).
    let start = client
        .call_tool(CallToolRequestParams::new("changes_since"))
        .await
        .unwrap();
    let start: serde_json::Value = serde_json::from_str(&text_of(&start)).unwrap();
    assert_eq!(start["changes"], serde_json::json!([]));
    let group = client
        .call_tool(
            CallToolRequestParams::new("create_task_group")
                .with_arguments(args(serde_json::json!({"name": "Cursor check"}))),
        )
        .await
        .unwrap();
    let group = text_of(&group);
    let after = client
        .call_tool(
            CallToolRequestParams::new("changes_since")
                .with_arguments(args(serde_json::json!({"cursor": start["cursor"]}))),
        )
        .await
        .unwrap();
    let after: serde_json::Value = serde_json::from_str(&text_of(&after)).unwrap();
    let rows = after["changes"].as_array().unwrap();
    assert!(
        rows.iter().any(|c| c["kind"] == "task_group"
            && c["key"] == serde_json::json!(group)
            && c["op"] == "created"),
        "{after}"
    );
    assert!(after["cursor"].as_i64().unwrap() > start["cursor"].as_i64().unwrap());

    // Bookmarks live in the vault, behind four tools (TICKET-037).
    let call = |name: &'static str, value: serde_json::Value| {
        let client = &client;
        async move {
            let result = client
                .call_tool(CallToolRequestParams::new(name).with_arguments(args(value)))
                .await
                .unwrap();
            serde_json::from_str::<serde_json::Value>(&text_of(&result)).unwrap()
        }
    };
    call(
        "bookmark_add",
        serde_json::json!({"kind": "file", "path": "ideas/Linker"}),
    )
    .await;
    let list = call(
        "bookmark_add",
        serde_json::json!({"kind": "search", "query": "tag:smoke", "title": "Smoke"}),
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 2, "{list}");
    let reordered = call(
        "bookmark_set",
        serde_json::json!({"bookmarks": [list[1].clone(), list[0].clone()]}),
    )
    .await;
    assert_eq!(reordered[0]["kind"], "search", "{reordered}");
    let left = call(
        "bookmark_remove",
        serde_json::json!({"kind": "search", "query": "tag:smoke"}),
    )
    .await;
    assert_eq!(
        left,
        serde_json::json!([{"kind": "file", "title": "Linker", "path": "ideas/Linker"}])
    );
    let listed = client
        .call_tool(CallToolRequestParams::new("bookmark_list"))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text_of(&listed)).unwrap(),
        left
    );
    assert!(home.join(".rusty/brain/.rusty/bookmarks.json").is_file());

    // A write on a row that is not there is an error that names it (TICKET-045); a
    // manager's error reaches the client as the call's error.
    let orphan = client
        .call_tool(
            CallToolRequestParams::new("create_task").with_arguments(args(
                serde_json::json!({"group_id": 987654, "title": "nowhere"}),
            )),
        )
        .await
        .unwrap_err();
    assert!(
        orphan.to_string().contains("No task group 987654"),
        "{orphan}"
    );

    // A link target makes the page at its path, folders and all (TICKET-041).
    let target = client
        .call_tool(
            CallToolRequestParams::new("brain_new_page")
                .with_arguments(args(serde_json::json!({"path": "decisions/smoke target"}))),
        )
        .await
        .unwrap();
    assert_eq!(
        text_of(&target),
        "\"decisions/smoke target\"",
        "{}",
        text_of(&target)
    );
    let linked = client
        .call_tool(
            CallToolRequestParams::new("brain_new_page").with_arguments(args(
                serde_json::json!({"folder": "ideas", "name": "Linker"}),
            )),
        )
        .await
        .unwrap();
    assert!(!linked.is_error.unwrap_or(false), "{}", text_of(&linked));
    let written = client
        .call_tool(
            CallToolRequestParams::new("brain_write_page").with_arguments(args(serde_json::json!({
                "slug": "ideas/Linker",
                "content": "---\ntitle: Linker\ntype: idea\n---\n\nSee [[projects/Smoke plan|the plan]].\n\n> [!tip] Hint\n> - [ ] a task\n"
            }))),
        )
        .await
        .unwrap();
    assert!(!written.is_error.unwrap_or(false), "{}", text_of(&written));
    let rendered = client
        .call_tool(
            CallToolRequestParams::new("brain_render").with_arguments(args(serde_json::json!({
                "slug": "ideas/Linker",
                "style": {"accent": "#123456"}
            }))),
        )
        .await
        .unwrap();
    let rendered_text = text_of(&rendered);
    assert!(
        rendered_text.contains("rusty:page/projects/Smoke plan"),
        "{rendered_text}"
    );
    assert!(rendered_text.contains("Hint"), "{rendered_text}");
    // Blocks only when asked; then typed, with the link resolved and the callout found
    // (TICKET-036).
    assert!(!rendered_text.contains("\"blocks\""), "{rendered_text}");
    let typed = client
        .call_tool(
            CallToolRequestParams::new("brain_render").with_arguments(args(serde_json::json!({
                "slug": "ideas/Linker",
                "blocks": true
            }))),
        )
        .await
        .unwrap();
    let typed: serde_json::Value = serde_json::from_str(&text_of(&typed)).unwrap();
    let blocks = typed["blocks"].as_array().unwrap();
    assert_eq!(blocks[0]["type"], "paragraph", "{typed}");
    assert_eq!(blocks[0]["inlines"][1]["type"], "wikilink", "{typed}");
    assert_eq!(
        blocks[0]["inlines"][1]["slug"], "projects/Smoke plan",
        "{typed}"
    );
    assert_eq!(blocks[1]["type"], "callout", "{typed}");
    assert_eq!(blocks[1]["kind"], "tip", "{typed}");
    assert!(typed["body_start"].as_u64().unwrap() > 0);
    assert!(rendered_text.contains("\"tasks\": 1"), "{rendered_text}");
    let moved = client
        .call_tool(
            CallToolRequestParams::new("brain_rename").with_arguments(args(serde_json::json!({
                "from": "projects/Smoke plan",
                "to": "concepts/Moved plan"
            }))),
        )
        .await
        .unwrap();
    let moved_text = text_of(&moved);
    assert!(
        moved_text.contains("\"pages_rewritten\": 1"),
        "{moved_text}"
    );
    let after = client
        .call_tool(
            CallToolRequestParams::new("brain_read_page")
                .with_arguments(args(serde_json::json!({"slug": "ideas/Linker"}))),
        )
        .await
        .unwrap();
    assert!(
        text_of(&after).contains("[[concepts/Moved plan|the plan]]"),
        "{}",
        text_of(&after)
    );
    // The page's file and the vault's folder come from the server, absolute (TICKET-042).
    let vault = home.canonicalize().unwrap().join(".rusty/brain");
    let page: serde_json::Value = serde_json::from_str(&text_of(&after)).unwrap();
    assert_eq!(
        page["file"],
        serde_json::json!(vault.join("ideas/Linker.md").to_string_lossy())
    );
    let stats = client
        .call_tool(CallToolRequestParams::new("brain_stats"))
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_str(&text_of(&stats)).unwrap();
    assert_eq!(
        stats["vault_root"],
        serde_json::json!(vault.to_string_lossy())
    );
    // The list carries aliases, and the properties asked for (TICKET-047).
    let listed = client
        .call_tool(
            CallToolRequestParams::new("brain_list_pages").with_arguments(args(
                serde_json::json!({"page_type": "idea", "properties": ["title", "nope"]}),
            )),
        )
        .await
        .unwrap();
    let listed: serde_json::Value = serde_json::from_str(&text_of(&listed)).unwrap();
    let linker = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["slug"] == "ideas/Linker")
        .unwrap();
    assert_eq!(linker["aliases"], serde_json::json!([]));
    assert_eq!(linker["properties"], serde_json::json!({"title": "Linker"}));
    let tree = client
        .call_tool(CallToolRequestParams::new("brain_tree"))
        .await
        .unwrap();
    let tree_text = text_of(&tree);
    assert!(
        tree_text.contains("\"path\": \"concepts/Moved plan\""),
        "{tree_text}"
    );
    let unresolved = client
        .call_tool(CallToolRequestParams::new("brain_unresolved"))
        .await
        .unwrap();
    assert!(
        text_of(&unresolved).trim() == "[]",
        "{}",
        text_of(&unresolved)
    );

    // Tags and properties: an inline tag reaches the tag list and the tag: search; a
    // property lands in the frontmatter with the body untouched, and leaves again.
    let tagged = client
        .call_tool(
            CallToolRequestParams::new("brain_write_page").with_arguments(args(serde_json::json!({
                "slug": "concepts/Moved plan",
                "content": "---\ntitle: Moved plan\ntype: concept\n---\n\nA plan with #smoke/test inside.\n"
            }))),
        )
        .await
        .unwrap();
    assert!(!tagged.is_error.unwrap_or(false), "{}", text_of(&tagged));
    let tags = client
        .call_tool(CallToolRequestParams::new("brain_tags"))
        .await
        .unwrap();
    let tags_text = text_of(&tags);
    assert!(
        tags_text.contains("\"tag\": \"smoke\"") && tags_text.contains("\"tag\": \"smoke/test\""),
        "{tags_text}"
    );
    let by_tag = client
        .call_tool(
            CallToolRequestParams::new("brain_search")
                .with_arguments(args(serde_json::json!({"query": "tag:smoke"}))),
        )
        .await
        .unwrap();
    assert!(
        text_of(&by_tag).contains("concepts/Moved plan"),
        "{}",
        text_of(&by_tag)
    );
    let by_pattern = client
        .call_tool(
            CallToolRequestParams::new("brain_search").with_arguments(args(
                serde_json::json!({"query": "path:concepts pl.n", "regex": true}),
            )),
        )
        .await
        .unwrap();
    assert!(
        text_of(&by_pattern).contains("concepts/Moved plan"),
        "{}",
        text_of(&by_pattern)
    );
    let set = client
        .call_tool(
            CallToolRequestParams::new("brain_set_property").with_arguments(args(
                serde_json::json!({
                    "slug": "concepts/Moved plan", "key": "status", "value": "active"
                }),
            )),
        )
        .await
        .unwrap();
    assert!(
        text_of(&set).contains("\"status\": \"active\""),
        "{}",
        text_of(&set)
    );
    let after = client
        .call_tool(
            CallToolRequestParams::new("brain_read_page")
                .with_arguments(args(serde_json::json!({"slug": "concepts/Moved plan"}))),
        )
        .await
        .unwrap();
    assert!(
        text_of(&after).contains("A plan with #smoke/test inside."),
        "{}",
        text_of(&after)
    );
    let graph = client
        .call_tool(
            CallToolRequestParams::new("brain_graph").with_arguments(args(
                serde_json::json!({"tags": true, "around": "ideas/Linker", "depth": 1}),
            )),
        )
        .await
        .unwrap();
    let graph_text = text_of(&graph);
    assert!(
        graph_text.contains("\"id\": \"concepts/Moved plan\""),
        "{graph_text}"
    );
    assert!(
        graph_text.contains("\"from\": \"ideas/Linker\""),
        "{graph_text}"
    );
    // Scripts as commands: list, view, update, run.
    let scripts = client
        .call_tool(
            CallToolRequestParams::new("script_list").with_arguments(args(serde_json::json!({}))),
        )
        .await
        .unwrap();
    let scripts_text = text_of(&scripts);
    assert!(
        scripts_text.contains("\"name\": \"usb-reset\""),
        "{scripts_text}"
    );
    let viewed = client
        .call_tool(
            CallToolRequestParams::new("script_view")
                .with_arguments(args(serde_json::json!({"name": "usb-reset"}))),
        )
        .await
        .unwrap();
    assert!(text_of(&viewed).contains("echo"), "{}", text_of(&viewed));
    client
        .call_tool(
            CallToolRequestParams::new("script_update").with_arguments(args(serde_json::json!({
                "name": "usb-reset", "body": "#!/usr/bin/env bash\necho \"bound $1\"\nexit 2\n"
            }))),
        )
        .await
        .unwrap();
    let ran = client
        .call_tool(
            CallToolRequestParams::new("script_run").with_arguments(args(
                serde_json::json!({"name": "usb-reset", "args": ["again"]}),
            )),
        )
        .await
        .unwrap();
    let ran: serde_json::Value = serde_json::from_str(&text_of(&ran)).unwrap();
    assert_eq!(ran["status"], 2, "{ran}");
    assert_eq!(ran["stdout"], "bound again\n", "{ran}");

    // The brain loop: ask, decide, due, follow up, and the typed edges in the graph.
    let asked = client
        .call_tool(
            CallToolRequestParams::new("brain_ask")
                .with_arguments(args(serde_json::json!({"question": "Moved plan"}))),
        )
        .await
        .unwrap();
    let asked: serde_json::Value = serde_json::from_str(&text_of(&asked)).unwrap();
    let consultation = asked["id"].as_str().unwrap().to_string();
    assert_eq!(consultation.len(), 32, "{asked}");
    assert!(
        asked["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["slug"] == "concepts/Moved plan"),
        "{asked}"
    );
    let decided = client
        .call_tool(
            CallToolRequestParams::new("brain_decide").with_arguments(args(serde_json::json!({
                "consultation": consultation,
                "title": "Keep the moved plan",
                "choice": "Keep it where it is",
                "rationale": "The links followed the move.",
                "alternatives": ["Move it back"],
                "follow_up_by": "2000-01-01"
            }))),
        )
        .await
        .unwrap();
    let decided: serde_json::Value = serde_json::from_str(&text_of(&decided)).unwrap();
    let decision_slug = decided["slug"].as_str().unwrap().to_string();
    assert!(decision_slug.starts_with("decisions/"), "{decided}");
    let due = client
        .call_tool(
            CallToolRequestParams::new("brain_due").with_arguments(args(serde_json::json!({}))),
        )
        .await
        .unwrap();
    let due: serde_json::Value = serde_json::from_str(&text_of(&due)).unwrap();
    assert_eq!(due["due"][0]["slug"], decision_slug, "{due}");
    assert_eq!(due["due"][0]["overdue"], true, "{due}");
    let followed = client
        .call_tool(
            CallToolRequestParams::new("brain_follow_up").with_arguments(args(serde_json::json!({
                "slug": decision_slug,
                "outcome": "Nobody missed the old place.",
                "status": "kept"
            }))),
        )
        .await
        .unwrap();
    assert!(
        text_of(&followed).contains("Follow-up"),
        "{}",
        text_of(&followed)
    );
    let loop_graph = client
        .call_tool(
            CallToolRequestParams::new("brain_graph").with_arguments(args(serde_json::json!({}))),
        )
        .await
        .unwrap();
    let loop_graph_text = text_of(&loop_graph);
    assert!(
        loop_graph_text.contains("\"kind\": \"consulted\""),
        "{loop_graph_text}"
    );
    let no_decision = client
        .call_tool(
            CallToolRequestParams::new("brain_no_decision").with_arguments(args(
                serde_json::json!({
                    "consultation": "absent", "reason": "nothing to decide"
                }),
            )),
        )
        .await;
    assert!(
        no_decision.is_err() || no_decision.as_ref().unwrap().is_error == Some(true),
        "an unknown consultation is refused"
    );
    let removed = client
        .call_tool(
            CallToolRequestParams::new("brain_remove_property").with_arguments(args(
                serde_json::json!({"slug": "concepts/Moved plan", "key": "status"}),
            )),
        )
        .await
        .unwrap();
    assert!(
        !text_of(&removed).contains("\"status\""),
        "{}",
        text_of(&removed)
    );

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// A `rusty-mcp` over stdio on its own scratch `HOME`.
async fn serve(home: &std::path::Path) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-mcp"));
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home.join("run"));
    ().serve(TokioChildProcess::new(cmd).expect("spawn rusty-mcp"))
        .await
        .expect("initialize")
}

/// Call `name` with `value`; the text of a success, or the error's message.
async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    value: serde_json::Value,
) -> Result<String, String> {
    match client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args(value)))
        .await
    {
        Ok(result) if !result.is_error.unwrap_or(false) => Ok(text_of(&result)),
        Ok(result) => Err(text_of(&result)),
        Err(e) => Err(e.to_string()),
    }
}

/// Setting and deleting a secret need the unlock once a PIN is set, and not before
/// (TICKET-049).
#[tokio::test]
async fn secret_writes_follow_the_pin() {
    let home = scratch_home();
    let client = serve(&home).await;

    call(
        &client,
        "secret_set",
        serde_json::json!({"key": "FIRST", "value": "one"}),
    )
    .await
    .expect("no PIN: a write needs no token");
    call(
        &client,
        "secret_set",
        serde_json::json!({"key": "SECOND", "value": "two"}),
    )
    .await
    .unwrap();
    call(
        &client,
        "secret_delete",
        serde_json::json!({"key": "SECOND"}),
    )
    .await
    .expect("no PIN: a delete needs no token");

    call(
        &client,
        "secret_pin_set",
        serde_json::json!({"pin": "123456"}),
    )
    .await
    .unwrap();
    let err = call(
        &client,
        "secret_set",
        serde_json::json!({"key": "FIRST", "value": "changed"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("locked"), "{err}");
    let err = call(
        &client,
        "secret_set",
        serde_json::json!({"key": "THIRD", "value": "three"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("locked"), "{err}");
    let err = call(
        &client,
        "secret_delete",
        serde_json::json!({"key": "FIRST"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("locked"), "{err}");
    let err = call(
        &client,
        "secret_delete",
        serde_json::json!({"key": "FIRST", "token": "not the token"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("unlock"), "{err}");
    let listed = call(&client, "secret_list", serde_json::json!({}))
        .await
        .unwrap();
    assert!(
        listed.contains("FIRST") && !listed.contains("THIRD"),
        "{listed}"
    );

    let unlocked = call(
        &client,
        "secret_unlock",
        serde_json::json!({"pin": "123456"}),
    )
    .await
    .unwrap();
    let token = serde_json::from_str::<serde_json::Value>(&unlocked).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    call(
        &client,
        "secret_set",
        serde_json::json!({"key": "FIRST", "value": "changed", "token": token}),
    )
    .await
    .unwrap();
    let revealed = call(
        &client,
        "secret_reveal",
        serde_json::json!({"key": "FIRST", "token": token}),
    )
    .await
    .unwrap();
    assert!(revealed.contains("changed"), "{revealed}");
    call(
        &client,
        "secret_delete",
        serde_json::json!({"key": "FIRST", "token": token}),
    )
    .await
    .unwrap();
    assert!(!call(&client, "secret_list", serde_json::json!({}))
        .await
        .unwrap()
        .contains("FIRST"));

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// No settings read returns a credential-looking value: `setting_get` masks what
/// `settings_list` masks (TICKET-050).
#[tokio::test]
async fn credential_settings_are_masked_on_every_read() {
    let home = scratch_home();
    let client = serve(&home).await;
    for (key, value) in [("provider_api_key", "sk-smoke"), ("theme_name", "dark")] {
        call(
            &client,
            "setting_set",
            serde_json::json!({"key": key, "value": value}),
        )
        .await
        .unwrap();
    }
    let masked = call(
        &client,
        "setting_get",
        serde_json::json!({"key": "provider_api_key"}),
    )
    .await
    .unwrap();
    assert_eq!(masked, "\"•••\"");
    let plain = call(
        &client,
        "setting_get",
        serde_json::json!({"key": "theme_name"}),
    )
    .await
    .unwrap();
    assert_eq!(plain, "\"dark\"");
    let unset = call(
        &client,
        "setting_get",
        serde_json::json!({"key": "unset_token"}),
    )
    .await
    .unwrap();
    assert_eq!(unset, "null");
    let listed = call(&client, "settings_list", serde_json::json!({}))
        .await
        .unwrap();
    assert!(
        !listed.contains("sk-smoke") && listed.contains("dark"),
        "{listed}"
    );
    let err = call(
        &client,
        "setting_set",
        serde_json::json!({"key": "provider_api_key", "value": "•••"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("mask"), "{err}");

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// The subjects of the skills store's commits, newest first.
fn store_log(home: &std::path::Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["log", "--format=%s"])
        .current_dir(home.join(".rusty/skills"))
        .output()
        .expect("run git log");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every skill write over MCP leaves a commit naming it before the call returns
/// (TICKET-051).
#[tokio::test]
async fn skill_writes_commit_the_store() {
    let home = scratch_home();
    std::fs::write(
        home.join(".gitconfig"),
        "[user]\n\tname = Smoke\n\temail = smoke@example.invalid\n",
    )
    .unwrap();
    let client = serve(&home).await;
    let body = "## Procedure\n\nSay hello.\n";
    let steps: [(&'static str, serde_json::Value, &str); 5] = [
        (
            "skill_create",
            serde_json::json!({"name": "smoke-active", "description": "An active one.", "body": body}),
            "skills: add smoke-active",
        ),
        (
            "skill_create",
            serde_json::json!({"name": "smoke-staged", "description": "A staged one.", "body": body, "pending": true}),
            "skills: stage smoke-staged",
        ),
        (
            "skill_approve",
            serde_json::json!({"name": "smoke-staged"}),
            "skills: approve smoke-staged",
        ),
        (
            "skill_delete",
            serde_json::json!({"name": "smoke-active"}),
            "skills: remove smoke-active",
        ),
        (
            "skill_create",
            serde_json::json!({"name": "smoke-rejected", "description": "Turned down.", "body": body, "pending": true}),
            "skills: stage smoke-rejected",
        ),
    ];
    for (tool, value, subject) in steps {
        call(&client, tool, value).await.unwrap();
        let log = store_log(&home);
        assert_eq!(log.first().map(String::as_str), Some(subject), "{log:?}");
    }
    call(
        &client,
        "skill_reject",
        serde_json::json!({"name": "smoke-rejected"}),
    )
    .await
    .unwrap();
    let log = store_log(&home);
    assert_eq!(
        log.first().map(String::as_str),
        Some("skills: reject smoke-rejected"),
        "{log:?}"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(home.join(".rusty/skills"))
        .output()
        .unwrap();
    assert!(status.stdout.is_empty(), "nothing left uncommitted");

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// A commit that fails leaves the write in place and the tool answers success
/// (TICKET-051).
#[tokio::test]
async fn a_failed_skill_commit_does_not_fail_the_tool() {
    let home = scratch_home();
    let store = home.join(".rusty/skills");
    std::fs::create_dir_all(&store).unwrap();
    // A `.git` that is not a repository: every git command in the store fails.
    std::fs::write(store.join(".git"), "not a repository\n").unwrap();
    let client = serve(&home).await;
    call(
        &client,
        "skill_create",
        serde_json::json!({"name": "smoke-uncommitted", "description": "Kept anyway.", "body": "## Procedure\n"}),
    )
    .await
    .expect("the write succeeds though the commit cannot");
    assert!(store
        .join(".claude/skills/smoke-uncommitted/SKILL.md")
        .is_file());

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// One set of importance words over MCP, `medium` read as `normal`, and the list in
/// level order (TICKET-052).
#[tokio::test]
async fn memory_importance_is_one_set_in_order() {
    let home = scratch_home();
    let client = serve(&home).await;
    for (content, importance) in [("first", "low"), ("second", "medium"), ("third", "high")] {
        call(
            &client,
            "store_memory",
            serde_json::json!({"content": content, "importance": importance}),
        )
        .await
        .unwrap();
    }
    call(
        &client,
        "store_memory",
        serde_json::json!({"content": "fourth"}),
    )
    .await
    .unwrap();
    let err = call(
        &client,
        "store_memory",
        serde_json::json!({"content": "refused", "importance": "urgent"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("low, normal or high"), "{err}");

    let listed = call(&client, "list_memories", serde_json::json!({}))
        .await
        .unwrap();
    let listed: Vec<serde_json::Value> = serde_json::from_str(&listed).unwrap();
    let levels: Vec<&str> = listed
        .iter()
        .map(|m| m["importance"].as_str().unwrap())
        .collect();
    assert_eq!(levels, ["high", "normal", "normal", "low"], "{listed:?}");
    assert!(listed.iter().all(|m| m["content"] != "refused"));

    let id = listed.iter().find(|m| m["content"] == "first").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let err = call(
        &client,
        "update_memory",
        serde_json::json!({"id": id, "importance": "urgent"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("low, normal or high"), "{err}");

    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// `search_conversations` finds a transcript that `rusty-cli ingest-conversation` kept.
#[tokio::test]
async fn search_conversations_finds_an_ingested_transcript() {
    use std::sync::Arc;
    let home = scratch_home();
    {
        let store = home.join(".rusty");
        std::fs::create_dir_all(&store).unwrap();
        let db =
            Arc::new(rusty_core::engine::db::Database::open_path(&store.join("rusty.db")).unwrap());
        let brain = Arc::new(rusty_core::brain::BrainManager::new(
            Arc::clone(&db),
            store.join("brain"),
        ));
        brain.ensure_vault().unwrap();
        let transcript = home.join("sid-smoke.jsonl");
        std::fs::write(
            &transcript,
            [
                r#"{"type":"ai-title","aiTitle":"Plan the quokka migration","sessionId":"sid-smoke"}"#,
                r#"{"type":"user","sessionId":"sid-smoke","cwd":"/proj","timestamp":"2026-06-27T10:00:00.000Z","message":{"role":"user","content":"We should migrate the Quokkanaut service to Rust."}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        rusty_core::engine::conversation_archive::ConversationArchive::new(db, brain)
            .ingest(&transcript)
            .unwrap();
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-mcp"));
    cmd.env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home.join("run"));
    let client =
        ().serve(TokioChildProcess::new(cmd).expect("spawn rusty-mcp"))
            .await
            .expect("initialize");
    let found = client
        .call_tool(
            CallToolRequestParams::new("search_conversations")
                .with_arguments(args(serde_json::json!({ "query": "Quokkanaut" }))),
        )
        .await
        .expect("search_conversations");
    let found: serde_json::Value = serde_json::from_str(&text_of(&found)).unwrap();
    let transcripts = found["transcripts"].as_array().expect("transcripts");
    assert_eq!(transcripts.len(), 1, "{found}");
    assert_eq!(transcripts[0]["session_id"], "sid-smoke");
    assert_eq!(found["agent_runs"].as_array().map(Vec::len), Some(0));
    client.cancel().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}
