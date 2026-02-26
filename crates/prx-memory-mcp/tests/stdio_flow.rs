use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[test]
fn memory_evolve_stdio_flow_works() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memory-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn prx-memory-mcp");

    let mut child_stdin = child.stdin.take().expect("stdin");
    let child_stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(child_stdout);

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "memory_evolve",
            "arguments": {
                "parent_score": 0.70,
                "candidates": [
                    {
                        "id": "v1",
                        "score_train": 0.80,
                        "score_holdout": 0.79,
                        "cost_penalty": 0.01,
                        "risk_penalty": 0.01,
                        "constraints_satisfied": true
                    },
                    {
                        "id": "v2",
                        "score_train": 0.85,
                        "score_holdout": 0.68,
                        "cost_penalty": 0.01,
                        "risk_penalty": 0.01,
                        "constraints_satisfied": true
                    }
                ]
            }
        }
    });

    writeln!(child_stdin, "{}", req).expect("write request");
    drop(child_stdin);

    let mut line = String::new();
    reader.read_line(&mut line).expect("read response line");

    let response: Value = serde_json::from_str(&line).expect("parse response json");
    let accepted = response["result"]["structuredContent"]["accepted_variant_id"]
        .as_str()
        .expect("accepted variant id");

    assert_eq!(accepted, "v1");

    let status = child.wait().expect("wait child");
    assert!(status.success());
}

fn write_framed(stdin: &mut std::process::ChildStdin, payload: &Value) {
    let body = serde_json::to_vec(payload).expect("serialize payload");
    let frame = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin
        .write_all(frame.as_bytes())
        .expect("write frame header");
    stdin.write_all(&body).expect("write frame body");
    stdin.flush().expect("flush frame");
}

fn read_framed(reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read frame header");
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }

    let len = content_length.expect("content-length header");
    let mut body = vec![0_u8; len];
    std::io::Read::read_exact(reader, &mut body).expect("read frame body");
    serde_json::from_slice(&body).expect("parse framed response")
}

#[test]
fn stdio_content_length_initialize_and_tools_list_work() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prx-memoryd"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn prx-memoryd");

    let mut child_stdin = child.stdin.take().expect("stdin");
    let child_stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(child_stdout);

    write_framed(
        &mut child_stdin,
        &json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "protocolVersion":"2024-11-05",
                "capabilities":{},
                "clientInfo":{"name":"stdio-test","version":"1.0.0"}
            }
        }),
    );
    let init = read_framed(&mut reader);
    assert_eq!(
        init["result"]["protocolVersion"].as_str(),
        Some("2024-11-05")
    );
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"].as_bool(),
        Some(false)
    );

    write_framed(
        &mut child_stdin,
        &json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/list",
            "params":{}
        }),
    );
    let tools = read_framed(&mut reader);
    let names = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(names.contains(&"memory_store"));
    assert!(names.contains(&"memory_recall"));

    drop(child_stdin);
    let status = child.wait().expect("wait child");
    assert!(status.success());
}
