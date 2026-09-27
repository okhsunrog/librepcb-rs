//! End-to-end test over stdio: runs the `librepcb-mcp` binary as child
//! process and talks to it with the rmcp client.

mod common;

use std::path::Path;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

async fn call(
    client: &RunningService<RoleClient, ()>,
    tool: &'static str,
    args: Value,
) -> CallToolResult {
    let args = match args {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    client
        .call_tool(CallToolRequestParams::new(tool).with_arguments(args))
        .await
        .unwrap_or_else(|e| panic!("{tool}: {e}"))
}

/// Calls a tool which must succeed; returns the `result` of the envelope.
async fn call_ok(
    client: &RunningService<RoleClient, ()>,
    tool: &'static str,
    args: Value,
) -> Value {
    let res = call(client, tool, args).await;
    let envelope = res.structured_content.clone().expect("structured content");
    assert_ne!(res.is_error, Some(true), "{tool} failed: {envelope:#}");
    assert_eq!(envelope["outcome"], "complete", "{tool}: {envelope:#}");
    assert!(
        matches!(res.content.first(), Some(ContentBlock::Text(_))),
        "{tool}: text summary first"
    );
    envelope["result"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn stdio_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_librepcb-mcp"));
    cmd.arg("--workspace").arg(&ws);
    let client = ().serve(TokioChildProcess::new(cmd).unwrap()).await.unwrap();

    // Tools.
    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "server_info",
        "workspace_open",
        "workspace_create",
        "project_create",
        "project_open",
        "project_save",
        "project_close",
        "project_summary",
        "library_list",
        "library_rescan",
        "library_install",
        "library_search",
        "library_element",
        "component_list",
        "component_get",
        "net_list",
        "netlist",
        "schematic_list",
        "schematic_get",
        "board_get",
        "erc_run",
        "export_fabrication",
        "export_bom",
        "export_pick_place",
        "export_netlist",
        "jobs_list",
        "render",
        "mutation_apply",
        "mutation_schema",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }
    assert!(names.len() < 60);
    for tool in &tools {
        assert!(tool.description.as_ref().is_some_and(|d| !d.is_empty()));
    }
    let search = tools.iter().find(|t| t.name == "library_search").unwrap();
    assert!(
        search
            .input_schema
            .get("properties")
            .unwrap()
            .get("query")
            .is_some()
    );

    // Workspace from the command line; add the upstream test library.
    let info = call_ok(&client, "server_info", json!({})).await;
    assert_eq!(info["units"]["length"], "mm");
    let libraries_path = info["workspace"]["libraries_path"]
        .as_str()
        .unwrap()
        .to_owned();
    common::install_populated_library(Path::new(&libraries_path));
    let scan = call_ok(&client, "library_rescan", json!({})).await;
    assert_eq!(scan["library_count"], 1);
    let found = call_ok(
        &client,
        "library_search",
        json!({ "query": "Diode", "kind": "component" }),
    )
    .await;
    let elements = found["elements"].as_array().unwrap();
    assert!(!elements.is_empty());
    let diode = elements[0]["uuid"].as_str().unwrap().to_owned();
    let detail = call_ok(&client, "library_element", json!({ "uuid": diode })).await;
    assert_eq!(detail["kind"], "component");
    let signals: Vec<&str> = detail["signals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(signals, ["A", "C"]);

    // New project in the workspace.
    let created = call_ok(&client, "project_create", json!({ "name": "E2E Test" })).await;
    assert_eq!(created["boards"][0]["name"], "default");
    let lpp = created["path"].as_str().unwrap().to_owned();
    assert!(Path::new(&lpp).is_file());
    call_ok(&client, "project_close", json!({})).await;

    // Upstream test project: read, ERC, Gerber, render.
    let project = common::copy_test_project("Nested Planes", tmp.path());
    call_ok(
        &client,
        "project_open",
        json!({ "path": project.to_string_lossy() }),
    )
    .await;
    let summary = call_ok(&client, "project_summary", json!({})).await;
    assert_eq!(summary["counts"]["components"], 9);
    let component = call_ok(&client, "component_get", json!({ "component": "R1" })).await;
    assert_eq!(component["designator"], "R1");

    let erc = call_ok(&client, "erc_run", json!({})).await;
    assert_eq!(erc["ran"], true);

    let gerber_dir = tmp.path().join("gerber");
    let fab = call_ok(
        &client,
        "export_fabrication",
        json!({ "output_dir": gerber_dir.to_string_lossy() }),
    )
    .await;
    let files = fab["files"].as_array().unwrap();
    assert!(files.iter().any(|f| f.as_str().unwrap().ends_with(".gbr")));
    assert!(
        files
            .iter()
            .all(|f| Path::new(f.as_str().unwrap()).is_file())
    );

    let res = call(
        &client,
        "render",
        json!({ "target": "board", "width": 320, "height": 240 }),
    )
    .await;
    assert_ne!(res.is_error, Some(true));
    let image = res
        .content
        .iter()
        .find_map(|c| match c {
            ContentBlock::Image(img) => Some(img.clone()),
            _ => None,
        })
        .expect("image content");
    assert_eq!(image.mime_type, "image/png");
    assert!(!image.data.is_empty());

    // Errors are tool results with a stable kind.
    let res = call(&client, "component_get", json!({ "component": "X99" })).await;
    assert_eq!(res.is_error, Some(true));
    let envelope = res.structured_content.unwrap();
    assert_eq!(envelope["outcome"], "failed");
    assert_eq!(envelope["error"]["kind"], "not_found");
    // Revision of the open project (the plane rebuild of the export
    // journals derived data changes).
    assert!(envelope["revision"].is_u64());

    call_ok(&client, "project_close", json!({})).await;
    let res = call(&client, "project_summary", json!({})).await;
    assert_eq!(
        res.structured_content.unwrap()["error"]["kind"],
        "no_project"
    );

    client.cancel().await.unwrap();
}
