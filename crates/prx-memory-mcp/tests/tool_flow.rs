use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use prx_memory_mcp::protocol::JsonRpcRequest;
use prx_memory_mcp::McpServer;
use serde_json::json;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_db_path() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir()
        .join(format!("prx-mcp-test-{pid}-{now}-{seq}.json"))
        .display()
        .to_string()
}

fn call_memory_store(
    server: &McpServer,
    id: u64,
    text: String,
    category: &str,
    importance_level: &str,
    governed: bool,
) -> serde_json::Value {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(id)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": text,
                "category": category,
                "scope": "global",
                "importance_level": importance_level,
                "governed": governed,
                "tags": ["project:prx-memory", "tool:mcp", "domain:maintenance"]
            }
        }),
    };
    server
        .handle_request(req)
        .expect("store response")
        .result
        .expect("store result")
}

#[test]
fn store_recall_forget_flow_works() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": "Pitfall: multilingual recall drifts. Cause: weak retrieval query prompt. Fix: use retrieval.query task. Prevention: keep query template stable.",
                "category": "fact",
                "scope": "global",
                "importance_level": "high",
                "governed": false,
                "tags": ["project:prx-memory", "tool:mcp", "domain:retrieval"]
            }
        }),
    };

    let store_resp = server.handle_request(store_req).expect("response");
    let stored_id = store_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .expect("stored id")
        .to_string();

    let recall_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(2)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_recall",
            "arguments": {
                "query": "multilingual retrieval query",
                "limit": 3
            }
        }),
    };

    let recall_resp = server.handle_request(recall_req).expect("response");
    let count = recall_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .expect("recall count");
    assert!(count >= 1);

    let forget_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(3)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_forget",
            "arguments": {
                "id": stored_id
            }
        }),
    };

    let forget_resp = server.handle_request(forget_req).expect("response");
    let deleted = forget_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("deleted"))
        .and_then(|v| v.as_bool())
        .expect("deleted flag");
    assert!(deleted);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn recall_remote_missing_key_returns_english_warning() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(10)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": "Pitfall: remote fallback warning unclear. Cause: missing provider key. Fix: show english warning. Prevention: check env before remote recall.",
                "category": "fact",
                "scope": "global",
                "importance_level": "medium",
                "governed": false,
                "tags": ["project:prx-memory", "tool:mcp", "domain:remote"]
            }
        }),
    };
    let _ = server.handle_request(store_req).expect("store response");

    let recall_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(11)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_recall",
            "arguments": {
                "query": "remote fallback",
                "limit": 3,
                "use_remote": true
            }
        }),
    };

    let recall_resp = server.handle_request(recall_req).expect("recall response");
    let warning = recall_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("warning"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    assert!(warning.contains("not configured") || warning.contains("Third-party"));
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn stats_and_list_tools_work() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(21)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": "Pitfall: noisy memories reduce precision. Cause: no governance gate. Fix: enforce template checks. Prevention: keep governed mode on.",
                "category": "fact",
                "scope": "global",
                "importance_level": "medium",
                "governed": false,
                "tags": ["project:prx-memory", "tool:mcp", "domain:governance"]
            }
        }),
    };
    let _ = server.handle_request(store_req).expect("store");

    let stats_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(22)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_stats",
            "arguments": {}
        }),
    };
    let stats_resp = server.handle_request(stats_req).expect("stats");
    let count = stats_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(count >= 1);

    let list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(23)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_list",
            "arguments": {"scope":"global","limit":5}
        }),
    };
    let list_resp = server.handle_request(list_req).expect("list");
    let listed = list_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(listed >= 1);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn resources_manifest_and_structured_tags_work() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let resources_list_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(24)),
        method: "resources/list".to_string(),
        params: json!({}),
    };
    let resources_list_resp = server
        .handle_request(resources_list_req)
        .expect("resources list");
    let resources = resources_list_resp
        .result
        .as_ref()
        .and_then(|v| v.get("resources"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(resources
        .iter()
        .any(|r| r.get("uri").and_then(|v| v.as_str())
            == Some("prx://skills/prx-memory-governance/SKILL.md")));

    let templates_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(241)),
        method: "resources/templates/list".to_string(),
        params: json!({}),
    };
    let templates_resp = server
        .handle_request(templates_req)
        .expect("resources templates list");
    let templates = templates_resp
        .result
        .as_ref()
        .and_then(|v| v.get("resourceTemplates"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(templates.iter().any(|r| {
        r.get("uriTemplate").and_then(|v| v.as_str())
            == Some("prx://templates/memory-store{?text,category,scope,importance_level}")
    }));

    let resource_read_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(25)),
        method: "resources/read".to_string(),
        params: json!({"uri":"prx://skills/prx-memory-governance/SKILL.md"}),
    };
    let resource_read_resp = server
        .handle_request(resource_read_req)
        .expect("resource read");
    let text = resource_read_resp
        .result
        .as_ref()
        .and_then(|v| v.get("contents"))
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(text.contains("PRX Memory Governance"));

    let template_read_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(251)),
        method: "resources/read".to_string(),
        params: json!({"uri":"prx://templates/memory-store?text=Pitfall:+template+smoke&scope=global"}),
    };
    let template_read_resp = server
        .handle_request(template_read_req)
        .expect("template read");
    let template_text = template_read_resp
        .result
        .as_ref()
        .and_then(|v| v.get("contents"))
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(template_text.contains("\"memory_store\""));

    let manifest_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(26)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_skill_manifest",
            "arguments":{"include_content": false}
        }),
    };
    let manifest_resp = server.handle_request(manifest_req).expect("manifest");
    let manifest_resources = manifest_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("resources"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(manifest_resources.len() >= 3);

    let dual_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(27)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_store_dual",
            "arguments":{
                "symptom":"tags fail policy without prefix fields",
                "cause":"raw tags are ambiguous",
                "fix":"supply explicit tag dimensions",
                "prevention":"always map project/tool/domain",
                "principle_tag":"tag-dimensions",
                "principle_rule":"use structured tag dimensions for governed writes",
                "trigger":"governed write input",
                "action":"send project_tag tool_tag domain_tag",
                "scope":"global",
                "governed":true,
                "project_tag":"prx-memory",
                "tool_tag":"mcp",
                "domain_tag":"governance"
            }
        }),
    };
    let dual_resp = server.handle_request(dual_req).expect("dual store");
    let dual_ok = dual_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("dual_layer_completed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(dual_ok);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn zero_config_store_defaults_work() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(801)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_store",
            "arguments":{
                "text":"Pitfall: zero config store path. Cause: caller omitted all optional fields. Fix: apply standardized defaults. Prevention: keep profile based defaults."
            }
        }),
    };
    let store_resp = server.handle_request(store_req).expect("store");
    let entry = store_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(entry.get("scope").and_then(|v| v.as_str()), Some("global"));
    assert_eq!(entry.get("category").and_then(|v| v.as_str()), Some("fact"));
    let tags = entry
        .get("tags")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let tag_texts = tags.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>();
    assert!(tag_texts.iter().any(|v| v.starts_with("project:")));
    assert!(tag_texts.iter().any(|v| v.starts_with("tool:")));
    assert!(tag_texts.iter().any(|v| v.starts_with("domain:")));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn governed_single_layer_store_is_rejected() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(28)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_store",
            "arguments":{
                "text":"Pitfall: single layer attempt. Cause: caller used legacy tool. Fix: switch to dual. Prevention: use memory_store_dual.",
                "category":"fact",
                "scope":"global",
                "importance_level":"high",
                "governed":true,
                "tags":["project:prx-memory","tool:mcp","domain:governance"]
            }
        }),
    };
    let resp = server.handle_request(req).expect("response");
    let err = resp
        .error
        .as_ref()
        .map(|e| e.message.clone())
        .unwrap_or_default();
    assert!(err.contains("memory_store_dual"));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn update_tool_replaces_memory() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(31)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": "Pitfall: ranking unstable. Cause: no fixed scoring baseline. Fix: add weighted score. Prevention: keep benchmark regression.",
                "category": "fact",
                "scope": "global",
                "importance_level": "medium",
                "governed": false,
                "tags": ["project:prx-memory", "tool:mcp", "domain:ranking"]
            }
        }),
    };
    let store_resp = server.handle_request(store_req).expect("store");
    let stored_id = store_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .expect("id")
        .to_string();

    let update_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(32)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_update",
            "arguments": {
                "id": stored_id,
                "text": "Pitfall: ranking unstable in long corpus. Cause: no fixed scoring baseline. Fix: add weighted score with cap. Prevention: keep benchmark regression.",
                "importance_level": "high",
                "tags": ["project:prx-memory", "tool:mcp", "domain:ranking"]
            }
        }),
    };
    let update_resp = server.handle_request(update_req).expect("update");
    let replaced = update_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("replaced_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(!replaced.is_empty());

    let recall_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(33)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_recall",
            "arguments": {
                "query": "long corpus weighted score",
                "limit": 5
            }
        }),
    };
    let recall_resp = server.handle_request(recall_req).expect("recall");
    let count = recall_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(count >= 1);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn maintenance_tools_flow_works() {
    let db_path = temp_db_path();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let export_path = std::env::temp_dir()
        .join(format!("prx-memory-export-{now}.json"))
        .display()
        .to_string();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let store_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(41)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_store",
            "arguments": {
                "text": "Pitfall: duplicate migration noise. Cause: repeated import without dedup. Fix: compact and skip duplicate. Prevention: run compact periodically.",
                "category": "fact",
                "scope": "global",
                "importance_level": "medium",
                "governed": false,
                "tags": ["project:prx-memory", "tool:mcp", "domain:maintenance"]
            }
        }),
    };
    let _ = server.handle_request(store_req).expect("store");

    let dup_import_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(411)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_import",
            "arguments":{
                "governed": false,
                "skip_duplicates": false,
                "entries":[
                    {
                        "text":"Pitfall: duplicate migration noise. Cause: repeated import without dedup. Fix: compact and skip duplicate. Prevention: run compact periodically.",
                        "category":"fact",
                        "scope":"global",
                        "importance_level":"medium",
                        "tags":["project:prx-memory", "tool:mcp", "domain:maintenance"]
                    }
                ]
            }
        }),
    };
    let _ = server.handle_request(dup_import_req).expect("dup import");

    let export_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(42)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_export",
            "arguments": {
                "scope":"global",
                "output_path": export_path
            }
        }),
    };
    let export_resp = server.handle_request(export_req).expect("export");
    let export_count = export_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(export_count >= 1);

    let compact_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(43)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_compact",
            "arguments":{"scope":"global","dry_run":false}
        }),
    };
    let compact_resp = server.handle_request(compact_req).expect("compact");
    let deleted = compact_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("deleted"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(deleted >= 1);

    let reembed_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(44)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_reembed",
            "arguments":{"scope":"global","limit":3}
        }),
    };
    let reembed_resp = server.handle_request(reembed_req).expect("reembed");
    let _updated = reembed_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("updated"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let db_path_2 = temp_db_path();
    let server2 = McpServer::with_db_path(&db_path_2).expect("server2");
    let migrate_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(45)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_migrate",
            "arguments":{"source_path": export_path, "governed": false}
        }),
    };
    let migrate_resp = server2.handle_request(migrate_req).expect("migrate");
    let migrated = migrate_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("created"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(migrated >= 1);

    let import_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(46)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_import",
            "arguments":{
                "governed": true,
                "entries":[
                    {
                        "text":"Pitfall: import path inconsistent. Cause: no standard format. Fix: use memory_import schema. Prevention: validate before import.",
                        "category":"fact",
                        "scope":"global",
                        "importance_level":"medium",
                        "tags":["project:prx-memory","tool:mcp","domain:maintenance"]
                    }
                ]
            }
        }),
    };
    let import_resp = server2.handle_request(import_req).expect("import");
    let created = import_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("created"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(created >= 1);

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path_2);
    let _ = std::fs::remove_file(export_path);
}

#[test]
fn dual_layer_store_tool_works() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let dual_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(51)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_store_dual",
            "arguments":{
                "symptom":"plugin update not taking effect",
                "cause":"runtime cache stale after restart",
                "fix":"clear cache before restart",
                "prevention":"add cache clean to restart script",
                "principle_tag":"plugin-cache",
                "principle_rule":"assume hidden cache for runtime-compiled code",
                "trigger":"code change not reflected",
                "action":"clear cache before debugging",
                "scope":"global",
                "tags":["project:prx-memory","tool:mcp","domain:governance"],
                "governed":true
            }
        }),
    };
    let dual_resp = server.handle_request(dual_req).expect("dual store");
    let dual_ok = dual_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("dual_layer_completed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(dual_ok);

    let recall_fact = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(52)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_recall",
            "arguments":{"query":"plugin cache stale restart","category":"fact","limit":3}
        }),
    };
    let fact_resp = server.handle_request(recall_fact).expect("fact recall");
    let fact_count = fact_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(fact_count >= 1);

    let recall_decision = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(53)),
        method: "tools/call".to_string(),
        params: json!({
            "name":"memory_recall",
            "arguments":{"query":"hidden cache runtime compiled code","category":"decision","limit":3}
        }),
    };
    let decision_resp = server
        .handle_request(recall_decision)
        .expect("decision recall");
    let decision_count = decision_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(decision_count >= 1);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn auto_compact_runs_on_100th_store() {
    let db_path = temp_db_path();
    let server = McpServer::with_db_path(&db_path).expect("server with temp db");

    let mut req_id = 60u64;
    for idx in 0..60 {
        let text = format!(
            "Pitfall: fact token tok{:03} retrieval noise. Cause: weak signal tok{:03}. Fix: strengthen anchor tok{:03}. Prevention: keep stable token tok{:03}.",
            idx, idx, idx, idx
        );
        let _ = call_memory_store(&server, req_id, text, "fact", "medium", false);
        req_id += 1;
    }

    for idx in 0..35 {
        let text = format!(
            "Decision principle (ratio-{:03}): keep strategy focused. Trigger: ratio token tok{:03}. Action: run gated update tok{:03}.",
            idx, idx, idx
        );
        let _ = call_memory_store(&server, req_id, text, "decision", "medium", false);
        req_id += 1;
    }

    let duplicate_text = "Pitfall: duplicate payload cluster. Cause: repeated import path. Fix: compact dedup. Prevention: periodic cleanup.".to_string();
    for _ in 0..4 {
        let _ = call_memory_store(
            &server,
            req_id,
            duplicate_text.clone(),
            "fact",
            "medium",
            false,
        );
        req_id += 1;
    }

    let trigger_result = call_memory_store(
        &server,
        req_id,
        "Pitfall: trigger checkpoint write. Cause: threshold hit. Fix: run maintenance. Prevention: periodic auto compact."
            .to_string(),
        "fact",
        "medium",
        false,
    );

    let auto_maintenance = trigger_result
        .get("structuredContent")
        .and_then(|v| v.get("auto_maintenance"))
        .expect("auto maintenance payload on 100th store");
    let duplicate_deleted = auto_maintenance
        .get("duplicate_deleted")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(duplicate_deleted >= 3);
    let rebalance_deleted = auto_maintenance
        .get("rebalance_deleted")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(rebalance_deleted >= 1);

    let stats_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(701)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_stats",
            "arguments": {"scope":"global"}
        }),
    };
    let stats_resp = server.handle_request(stats_req).expect("stats");
    let ratio = stats_resp
        .result
        .as_ref()
        .and_then(|v| v.get("structuredContent"))
        .and_then(|v| v.get("decision_ratio"))
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    assert!(ratio <= 0.30 + 1e-6, "decision_ratio={ratio}");

    let _ = std::fs::remove_file(db_path);
}
