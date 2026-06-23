//! Tests for MCP config and protocol foundations.

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use serde_json::json;

use super::{
    client::discover_http_server,
    config::{parse_mcp_config_json, McpConfigError, McpInternalServerKind, McpServerConfig},
    logging::McpLogContext,
    protocol::{
        initialize_request, initialized_notification, resources_list_request, tools_list_request,
        JsonRpcResponse, McpTool, ToolsListResult, MCP_PROTOCOL_VERSION,
    },
};

#[test]
fn parses_http_and_stdio_config() {
    let config = parse_mcp_config_json(
        r#"{
          "servers": {
            "context7": {
              "transport": "http",
              "url": "https://mcp.context7.com/mcp",
              "headers": {
                "CONTEXT7_API_KEY": { "env": "CONTEXT7_API_KEY" }
              }
            },
            "obsidian": {
              "transport": "stdio",
              "command": "obsidian-mcp-server",
              "args": ["--vault", "Notes"],
              "env": {
                "OBSIDIAN_TOKEN": { "env": "OBSIDIAN_TOKEN" }
              }
            }
          }
        }"#,
    )
    .expect("valid MCP config should parse");

    assert!(matches!(
        config.servers.get("context7"),
        Some(McpServerConfig::Http(_))
    ));
    assert!(matches!(
        config.servers.get("obsidian"),
        Some(McpServerConfig::Stdio(_))
    ));
}

#[test]
fn parses_internal_project_index_config() {
    let config = parse_mcp_config_json(
        r#"{
          "servers": {
            "project-index": {
              "transport": "internal",
              "kind": "project_index"
            }
          }
        }"#,
    )
    .expect("valid internal MCP config should parse");

    assert!(matches!(
        config.servers.get("project-index"),
        Some(McpServerConfig::Internal(config))
            if config.kind == McpInternalServerKind::ProjectIndex
    ));
}

#[test]
fn rejects_unsupported_http_url_scheme() {
    let error = parse_mcp_config_json(
        r#"{
          "servers": {
            "bad": {
              "transport": "http",
              "url": "ftp://example.com/mcp"
            }
          }
        }"#,
    )
    .expect_err("unsupported URL scheme should fail");

    assert!(matches!(error, McpConfigError::UnsupportedHttpUrl { .. }));
}

#[test]
fn rejects_empty_stdio_command() {
    let error = parse_mcp_config_json(
        r#"{
          "servers": {
            "obsidian": {
              "transport": "stdio",
              "command": " "
            }
          }
        }"#,
    )
    .expect_err("empty stdio command should fail");

    assert!(matches!(error, McpConfigError::EmptyStdioCommand { .. }));
}

#[test]
fn builds_initialize_request() {
    let request = initialize_request(1, "elgar", "0.10.0");
    let rendered = serde_json::to_value(request).expect("request should serialize");

    assert_eq!(rendered["jsonrpc"], "2.0");
    assert_eq!(rendered["id"], 1);
    assert_eq!(rendered["method"], "initialize");
    assert_eq!(rendered["params"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    assert_eq!(rendered["params"]["clientInfo"]["name"], "elgar");
}

#[test]
fn builds_initialized_notification() {
    let notification = initialized_notification();
    let rendered = serde_json::to_value(notification).expect("notification should serialize");

    assert_eq!(rendered["jsonrpc"], "2.0");
    assert_eq!(rendered["method"], "notifications/initialized");
}

#[test]
fn builds_list_requests() {
    let tools = tools_list_request(2, Some("next".to_string()));
    let resources = resources_list_request(3, None);

    assert_eq!(tools.method, "tools/list");
    assert_eq!(tools.params.cursor.as_deref(), Some("next"));
    assert_eq!(resources.method, "resources/list");
    assert_eq!(resources.params.cursor, None);
}

#[test]
fn parses_tools_list_response() {
    let response: JsonRpcResponse<ToolsListResult> = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "tools": [
                {
                    "name": "query-docs",
                    "description": "Retrieve documentation",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        }
                    }
                }
            ]
        }
    }))
    .expect("tools response should parse");

    assert_eq!(response.result.tools.len(), 1);
    assert_eq!(
        response.result.tools.first(),
        Some(&McpTool {
            name: "query-docs".to_string(),
            title: None,
            description: Some("Retrieve documentation".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        })
    );
}

#[test]
fn discovers_http_server_with_fake_mcp_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake MCP server");
    let addr = listener.local_addr().expect("fake server addr");
    let handle = thread::spawn(move || {
        for index in 0..4 {
            let (mut stream, _) = listener.accept().expect("accept fake MCP request");
            let request = read_fake_http_request(&mut stream);
            let body = fake_mcp_response_body(index, &request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write fake response");
        }
    });

    let config = match parse_mcp_config_json(&format!(
        r#"{{
          "servers": {{
            "fake": {{
              "transport": "http",
              "url": "http://{addr}/mcp",
              "timeout_millis": 1000
            }}
          }}
        }}"#
    ))
    .expect("fake MCP config parses")
    .servers
    .remove("fake")
    .expect("fake server exists")
    {
        McpServerConfig::Http(config) => config,
        McpServerConfig::Stdio(_) | McpServerConfig::Internal(_) => {
            panic!("expected HTTP config")
        }
    };

    let discovery = discover_http_server(&config, Default::default(), "test", None)
        .expect("MCP discovery works");

    assert_eq!(discovery.initialize.server_info.name, "fake-mcp");
    assert_eq!(
        discovery
            .tools
            .expect("tools should be listed")
            .tools
            .first()
            .map(|tool| tool.name.as_str()),
        Some("query-docs")
    );
    assert_eq!(
        discovery
            .resources
            .expect("resources should be listed")
            .resources
            .first()
            .map(|resource| resource.name.as_str()),
        Some("docs-index")
    );

    handle.join().expect("fake server thread joins");
}

#[test]
fn logs_http_mcp_discovery_events_without_header_values() {
    std::env::set_var("ELGAR_LOG", "1");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake MCP server");
    let addr = listener.local_addr().expect("fake server addr");
    let handle = thread::spawn(move || {
        for index in 0..4 {
            let (mut stream, _) = listener.accept().expect("accept fake MCP request");
            let request = read_fake_http_request(&mut stream);
            let body = fake_mcp_response_body(index, &request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write fake response");
        }
    });
    let temp = std::env::temp_dir().join(format!("elgar-mcp-log-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).expect("create temp log root");

    let config = match parse_mcp_config_json(&format!(
        r#"{{
          "servers": {{
            "fake": {{
              "transport": "http",
              "url": "http://{addr}/mcp",
              "headers": {{
                "Authorization": {{ "env": "SHOULD_NOT_BE_READ_IN_THIS_TEST" }}
              }},
              "timeout_millis": 1000
            }}
          }}
        }}"#
    ))
    .expect("fake MCP config parses")
    .servers
    .remove("fake")
    .expect("fake server exists")
    {
        McpServerConfig::Http(config) => config,
        McpServerConfig::Stdio(_) | McpServerConfig::Internal(_) => {
            panic!("expected HTTP config")
        }
    };

    let mut headers = std::collections::BTreeMap::new();
    headers.insert("Authorization".to_string(), "super-secret".to_string());
    let log_context = McpLogContext {
        project_root: temp.clone(),
        session_id: "mcp-log-test".to_string(),
        turn_id: 0,
        server_id: "fake".to_string(),
        transport: "http".to_string(),
    };

    let discovery =
        discover_http_server(&config, headers, "test", Some(log_context)).expect("MCP discovery");

    assert_eq!(discovery.initialize.server_info.name, "fake-mcp");
    let log_path = temp.join(".elgar/log/system/mcp-log-test.jsonl");
    let logs = fs::read_to_string(&log_path).expect("read MCP system log");
    assert!(logs.contains(r#""summary":"mcp_http_request_started""#));
    assert!(logs.contains(r#""summary":"mcp_http_request_finished""#));
    assert!(logs.contains(r#""summary":"mcp_initialize_finished""#));
    assert!(logs.contains(r#""summary":"mcp_tools_listed""#));
    assert!(logs.contains(r#""summary":"mcp_resources_listed""#));
    assert!(!logs.contains("super-secret"));

    handle.join().expect("fake server thread joins");
    let _ = fs::remove_dir_all(temp);
    std::env::remove_var("ELGAR_LOG");
}

fn read_fake_http_request(stream: &mut std::net::TcpStream) -> String {
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

fn fake_mcp_response_body(index: usize, request: &str) -> String {
    match index {
        0 => {
            assert!(request.contains(r#""method":"initialize""#));
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"fake-mcp","version":"1.0.0"}}}"#.to_string()
        }
        1 => {
            assert!(request.contains(r#""method":"notifications/initialized""#));
            "{}".to_string()
        }
        2 => {
            assert!(request.contains(r#""method":"tools/list""#));
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"query-docs","description":"Retrieve documentation","inputSchema":{"type":"object"}}]}}"#.to_string()
        }
        3 => {
            assert!(request.contains(r#""method":"resources/list""#));
            r#"{"jsonrpc":"2.0","id":3,"result":{"resources":[{"uri":"context7://docs","name":"docs-index"}]}}"#.to_string()
        }
        _ => panic!("unexpected fake request"),
    }
}
