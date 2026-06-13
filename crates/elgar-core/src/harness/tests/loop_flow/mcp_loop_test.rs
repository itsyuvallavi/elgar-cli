//! MCP capability behavior in the native harness loop.

use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use crate::{
    event::ProviderOutput, harness::run_primitive_harness_loop, provider::ChatRole,
    session::Session,
};

use super::{
    super::support::queued_provider::QueuedProvider,
    loop_helpers::{tool_call_output, tool_message_contents},
};

#[test]
fn mcp_call_schema_is_hidden_without_mcp_config() {
    let root = temp_root("mcp-hidden");
    let provider = QueuedProvider::new_outputs(vec![ProviderOutput::new("No MCP needed.")]);
    let mut session = Session::new("mcp-hidden-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "hello").unwrap();

    assert_eq!(result.stopped_reason, "model_message");
    let tool_calls = provider.tool_calls.lock().expect("tool calls lock");
    assert!(tool_calls[0]
        .iter()
        .all(|tool| tool.function.name != "mcp_call"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn mcp_call_schema_is_visible_with_mcp_config() {
    let root = temp_root("mcp-visible");
    fs::write(
        root.join("elgar-mcp.json"),
        r#"{"servers":{"context7":{"transport":"http","url":"http://127.0.0.1:9/mcp"}}}"#,
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![ProviderOutput::new("No MCP needed.")]);
    let mut session = Session::new("mcp-visible-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "hello").unwrap();

    assert_eq!(result.stopped_reason, "model_message");
    let tool_calls = provider.tool_calls.lock().expect("tool calls lock");
    assert!(tool_calls[0]
        .iter()
        .any(|tool| tool.function.name == "mcp_call"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_mcp_call_returns_verified_evidence() {
    let root = temp_root("mcp-native-call");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake MCP server");
    let addr = listener.local_addr().expect("fake MCP addr");
    fs::write(
        root.join("elgar-mcp.json"),
        format!(
            r#"{{"servers":{{"context7":{{"transport":"http","url":"http://{addr}/mcp","timeout_millis":1000}}}}}}"#
        ),
    )
    .unwrap();
    let handle = spawn_fake_mcp_server(listener, 7);
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "mcp_call",
            r#"{"server":"context7","tool":"query-docs","arguments":{"libraryId":"/vercel/next.js","query":"middleware auth"}}"#,
            "call-mcp-docs",
        ),
        ProviderOutput::new("Used Context7 docs."),
    ]);
    let mut session = Session::new("mcp-native-call-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "use Context7 docs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_results = tool_message_contents(&calls[1]);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert!(tool_results.iter().any(|content| {
        content.contains("VERIFIED_MCP_TOOL_RESULT")
            && content.contains("server: context7")
            && content.contains("tool: query-docs")
            && content.contains("Next.js middleware docs")
    }));
    assert!(calls[1]
        .iter()
        .any(|message| matches!(message.role, ChatRole::Tool)
            && message.tool_call_id.as_deref() == Some("call-mcp-docs")));

    handle.join().expect("fake MCP server joins");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_mcp_prompt_includes_active_tool_catalog() {
    let root = temp_root("mcp-prompt-catalog");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake MCP server");
    let addr = listener.local_addr().expect("fake MCP addr");
    fs::write(
        root.join("elgar-mcp.json"),
        format!(
            r#"{{"servers":{{"context7":{{"transport":"http","url":"http://{addr}/mcp","timeout_millis":1000}}}}}}"#
        ),
    )
    .unwrap();
    let handle = spawn_fake_mcp_server(listener, 3);
    let provider = QueuedProvider::new_outputs(vec![ProviderOutput::new("No MCP needed.")]);
    let mut session = Session::new("mcp-prompt-catalog-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "use Context7 docs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let system_prompt = &calls[0][0].content;

    assert_eq!(result.stopped_reason, "model_message");
    assert!(system_prompt.contains("Active MCP tools"));
    assert!(system_prompt.contains("server: context7"));
    assert!(system_prompt.contains("tool: resolve-library-id"));
    assert!(system_prompt.contains("tool: query-docs"));
    assert!(system_prompt.contains("libraryId"));

    handle.join().expect("fake MCP server joins");
    let _ = fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-mcp-loop-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn spawn_fake_mcp_server(listener: TcpListener, request_count: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("accept fake MCP request");
            let request = read_fake_http_request(&mut stream);
            let body = fake_mcp_response_body(&request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write fake MCP response");
        }
    })
}

fn read_fake_http_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).expect("read fake request");
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..split]);
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if bytes.len() >= split + 4 + content_length {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

fn fake_mcp_response_body(request: &str) -> String {
    if request.contains(r#""method":"initialize""#) {
        return r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake-context7","version":"1.0.0"}}}"#.to_string();
    }
    if request.contains(r#""method":"notifications/initialized""#) {
        return "{}".to_string();
    }
    if request.contains(r#""method":"tools/list""#) {
        return r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"resolve-library-id","description":"Resolve a package name into a Context7-compatible library ID.","inputSchema":{"type":"object","properties":{"libraryName":{"type":"string"}},"required":["libraryName"]}},{"name":"query-docs","description":"Retrieve documentation for a resolved library ID.","inputSchema":{"type":"object","properties":{"libraryId":{"type":"string"},"query":{"type":"string"}},"required":["libraryId","query"]}}]}}"#.to_string();
    }
    if request.contains(r#""method":"tools/call""#) {
        assert!(request.contains("middleware auth"));
        return r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"Next.js middleware docs: authenticate in middleware before route handling."}],"isError":false}}"#.to_string();
    }
    panic!("unexpected fake MCP request: {request}")
}
