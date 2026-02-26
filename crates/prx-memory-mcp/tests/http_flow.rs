use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn reserve_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve addr");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    addr.to_string()
}

fn wait_for_http(addr: &str) {
    for _ in 0..80 {
        if TcpStream::connect(addr).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("http server not ready on {addr}");
}

fn send_http(addr: &str, method: &str, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect http");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes()).expect("write request");
    stream.flush().expect("flush");
    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read response");
    buf
}

fn response_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn response_header(response: &str) -> &str {
    response.split("\r\n\r\n").next().unwrap_or("")
}

#[test]
fn http_health_and_mcp_call_work() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let db_path = std::env::temp_dir()
        .join(format!("prx-memory-http-{now}.json"))
        .display()
        .to_string();
    let addr = reserve_addr();

    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memoryd"))
        .env("PRX_MEMORYD_TRANSPORT", "http")
        .env("PRX_MEMORY_HTTP_ADDR", &addr)
        .env("PRX_MEMORY_DB", &db_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn prx-memoryd");

    wait_for_http(&addr);

    let health = send_http(&addr, "GET", "/health", "");
    assert!(health.starts_with("HTTP/1.1 200"));
    assert!(response_body(&health).contains("\"status\":\"ok\""));

    let init_body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let init = send_http(&addr, "POST", "/mcp", init_body);
    assert!(init.starts_with("HTTP/1.1 200"));
    let body = response_body(&init);
    assert!(body.contains("\"jsonrpc\":\"2.0\""));
    assert!(body.contains("\"serverInfo\""));
    assert!(body.contains("\"prx-memory-mcp\""));

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn http_stream_session_and_metrics_work() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let db_path = std::env::temp_dir()
        .join(format!("prx-memory-http-stream-{now}.json"))
        .display()
        .to_string();
    let addr = reserve_addr();

    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memoryd"))
        .env("PRX_MEMORYD_TRANSPORT", "http")
        .env("PRX_MEMORY_HTTP_ADDR", &addr)
        .env("PRX_MEMORY_DB", &db_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn prx-memoryd");

    wait_for_http(&addr);

    let start_resp = send_http(&addr, "POST", "/mcp/session/start", "{}");
    assert!(start_resp.starts_with("HTTP/1.1 200"));
    let start_json: serde_json::Value =
        serde_json::from_str(response_body(&start_resp)).expect("start json");
    let session_id = start_json
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    let init_body = r#"{"jsonrpc":"2.0","id":11,"method":"initialize","params":{}}"#;
    let enqueue = send_http(
        &addr,
        "POST",
        &format!("/mcp/stream?session={session_id}"),
        init_body,
    );
    assert!(enqueue.starts_with("HTTP/1.1 202"));

    let poll = send_http(
        &addr,
        "GET",
        &format!("/mcp/stream?session={session_id}&from=1&limit=10"),
        "",
    );
    assert!(poll.starts_with("HTTP/1.1 200"));
    let poll_json: serde_json::Value = serde_json::from_str(response_body(&poll)).expect("poll");
    let count = poll_json.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(count >= 1);

    let tool_req = r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"memory_stats","arguments":{}}}"#;
    let tool_resp = send_http(&addr, "POST", "/mcp", tool_req);
    assert!(tool_resp.starts_with("HTTP/1.1 200"));

    let metrics = send_http(&addr, "GET", "/metrics", "");
    assert!(metrics.starts_with("HTTP/1.1 200"));
    let metrics_body = response_body(&metrics);
    assert!(metrics_body.contains("prx_memory_tool_calls_total"));
    assert!(metrics_body.contains("memory_stats"));
    assert!(metrics_body.contains("prx_memory_embed_cache_hits_total"));
    assert!(metrics_body.contains("prx_memory_metrics_label_overflow_total"));
    assert!(metrics_body.contains("prx_memory_alert_state"));

    let summary = send_http(&addr, "GET", "/metrics/summary", "");
    assert!(summary.starts_with("HTTP/1.1 200"));
    let summary_json: serde_json::Value =
        serde_json::from_str(response_body(&summary)).expect("summary");
    assert_eq!(
        summary_json.get("status").and_then(|v| v.as_str()),
        Some("ok")
    );
    assert!(summary_json.get("overall_alert_level").is_some());
    assert!(summary_json.get("cardinality_limits").is_some());

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn http_stream_ack_and_sse_work() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let db_path = std::env::temp_dir()
        .join(format!("prx-memory-http-sse-{now}.json"))
        .display()
        .to_string();
    let addr = reserve_addr();

    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memoryd"))
        .env("PRX_MEMORYD_TRANSPORT", "http")
        .env("PRX_MEMORY_HTTP_ADDR", &addr)
        .env("PRX_MEMORY_DB", &db_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn prx-memoryd");

    wait_for_http(&addr);

    let start_resp = send_http(&addr, "POST", "/mcp/session/start", "{}");
    let start_json: serde_json::Value =
        serde_json::from_str(response_body(&start_resp)).expect("start json");
    let session_id = start_json
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    let req1 = r#"{"jsonrpc":"2.0","id":21,"method":"initialize","params":{}}"#;
    let req2 = r#"{"jsonrpc":"2.0","id":22,"method":"tools/list","params":{}}"#;
    let enqueue1 = send_http(
        &addr,
        "POST",
        &format!("/mcp/stream?session={session_id}"),
        req1,
    );
    let enqueue2 = send_http(
        &addr,
        "POST",
        &format!("/mcp/stream?session={session_id}"),
        req2,
    );
    assert!(enqueue1.starts_with("HTTP/1.1 202"));
    assert!(enqueue2.starts_with("HTTP/1.1 202"));

    let poll = send_http(
        &addr,
        "GET",
        &format!("/mcp/stream?session={session_id}&from=1&limit=10&ack=1"),
        "",
    );
    assert!(poll.starts_with("HTTP/1.1 200"));
    let poll_json: serde_json::Value = serde_json::from_str(response_body(&poll)).expect("poll");
    assert_eq!(
        poll_json.get("effective_from").and_then(|v| v.as_u64()),
        Some(2)
    );
    assert_eq!(
        poll_json.get("ack_applied").and_then(|v| v.as_u64()),
        Some(1)
    );
    let events = poll_json
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].get("seq").and_then(|v| v.as_u64()), Some(2));

    let sse = send_http(
        &addr,
        "GET",
        &format!(
            "/mcp/stream?session={session_id}&mode=sse&from=2&limit=1&wait_ms=300&heartbeat_ms=100"
        ),
        "",
    );
    assert!(sse.starts_with("HTTP/1.1 200"));
    assert!(response_header(&sse).contains("text/event-stream"));
    let sse_body = response_body(&sse);
    assert!(sse_body.contains("event: message"));
    assert!(sse_body.contains("\"seq\":2"));
    assert!(sse_body.contains("event: cursor"));

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn http_stream_session_expiry_work() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let db_path = std::env::temp_dir()
        .join(format!("prx-memory-http-expire-{now}.json"))
        .display()
        .to_string();
    let addr = reserve_addr();

    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memoryd"))
        .env("PRX_MEMORYD_TRANSPORT", "http")
        .env("PRX_MEMORY_HTTP_ADDR", &addr)
        .env("PRX_MEMORY_DB", &db_path)
        .env("PRX_MEMORY_STREAM_SESSION_TTL_MS", "1000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn prx-memoryd");

    wait_for_http(&addr);

    let start_resp = send_http(&addr, "POST", "/mcp/session/start", "{}");
    let start_json: serde_json::Value =
        serde_json::from_str(response_body(&start_resp)).expect("start json");
    let session_id = start_json
        .get("session_id")
        .and_then(|v| v.as_str())
        .expect("session id")
        .to_string();

    std::thread::sleep(Duration::from_millis(1200));
    let poll = send_http(
        &addr,
        "GET",
        &format!("/mcp/stream?session={session_id}&from=1&limit=1"),
        "",
    );
    assert!(poll.starts_with("HTTP/1.1 404") || poll.starts_with("HTTP/1.1 410"));
    let body = response_body(&poll);
    assert!(body.contains("session_expired") || body.contains("session_not_found"));

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(db_path);
}
