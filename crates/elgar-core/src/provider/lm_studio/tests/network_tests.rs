//! LM Studio local network behavior tests.

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use super::super::{
    chat_lm_studio, chat_lm_studio_streaming, ChatMessage, LmStudioProvider, ProviderConfig,
};
use crate::provider::{
    ControllerProvider, ProviderCancelToken, ProviderErrorKind, ProviderStreamChunk,
};

fn write_chunk(stream: &mut std::net::TcpStream, body: &str) {
    write!(stream, "{:x}\r\n{}\r\n", body.len(), body).unwrap();
    stream.flush().unwrap();
}

#[test]
fn non_streaming_message_request_overrides_streaming_config() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let bytes_read = stream.read(&mut request).unwrap();
        sender
            .send(String::from_utf8_lossy(&request[..bytes_read]).to_string())
            .unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
                {\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"Hello.\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}",
            )
            .unwrap();
    });

    let provider = LmStudioProvider::new(ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    });
    let metadata = provider.request_metadata();
    let output = provider
        .chat_messages_without_streaming_with_metadata(vec![ChatMessage::user("hello")], &metadata)
        .unwrap();

    server.join().unwrap();
    let request = receiver.recv().unwrap();
    assert!(request.contains(r#""stream":false"#));
    assert_eq!(output.metrics.unwrap().usage.unwrap().total_tokens, Some(7));
}

#[test]
fn live_streaming_chat_emits_reasoning_and_response_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"reasoning_content":"Need greet."}}]}

"#,
        );
        thread::sleep(Duration::from_millis(5));
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}

"#,
        );
        write_chunk(&mut stream, "data: [DONE]\n\n");
        stream.write_all(b"0\r\n\r\n").unwrap();
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let output =
        chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
            chunks.push(chunk);
        })
        .unwrap();

    server.join().unwrap();
    assert_eq!(output.text, "Hello");
    assert_eq!(output.thinking.as_deref(), Some("Need greet."));
    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("Need greet.".to_string()),
            ProviderStreamChunk::Text("Hello".to_string())
        ]
    );
}

#[test]
fn live_streaming_chat_rejects_incomplete_chunked_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Partial"}}]}

"#,
        );
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let error = chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
        chunks.push(chunk);
    })
    .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
    assert!(error.message.contains("terminal chunk"));
    assert_eq!(
        chunks,
        vec![ProviderStreamChunk::Text("Partial".to_string())]
    );
}

#[test]
fn live_chat_reports_read_timeout_with_phase_without_external_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        thread::sleep(Duration::from_millis(40));
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        connect_timeout_millis: Some(1_000),
        read_timeout_millis: Some(10),
        request_timeout_millis: Some(1_000),
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let error = chat_lm_studio(&config, vec![ChatMessage::user("hello")]).unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::Network);
    assert!(error.message.contains("provider read timed out"));
}

#[test]
fn live_chat_cancellation_aborts_blocked_read_before_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        thread::sleep(Duration::from_millis(600));
    });

    let provider = LmStudioProvider::new(ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        connect_timeout_millis: Some(1_000),
        read_timeout_millis: Some(1_000),
        request_timeout_millis: Some(5_000),
        ..ProviderConfig::lm_studio("loaded-model")
    });
    let metadata = provider.request_metadata();
    let cancel = ProviderCancelToken::new();
    let cancel_worker = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        cancel_worker.cancel();
    });

    let started = std::time::Instant::now();
    let error = provider
        .chat_messages_without_streaming_with_metadata_cancelable(
            vec![ChatMessage::user("hello")],
            &metadata,
            &cancel,
        )
        .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::Canceled);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn live_streaming_timeout_after_partial_chunk_returns_no_finished_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Partial"}}]}

"#,
        );
        thread::sleep(Duration::from_millis(40));
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        connect_timeout_millis: Some(1_000),
        read_timeout_millis: Some(10),
        request_timeout_millis: Some(1_000),
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let error = chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
        chunks.push(chunk);
    })
    .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::Network);
    assert!(error.message.contains("provider stream read timed out"));
    assert_eq!(
        chunks,
        vec![ProviderStreamChunk::Text("Partial".to_string())]
    );
}
