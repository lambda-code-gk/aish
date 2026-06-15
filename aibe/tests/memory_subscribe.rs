//! MemorySubscribe broker と socket 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::{
    shared_builtin_loader, FilesystemContextualMemoryStore, FilesystemMemorySpaceResolver,
    InProcessMemorySubscriptionBroker,
};
use aibe::application::memory_service::MemoryService;
use aibe::application::server;
use aibe::domain::{MemoryChangeEvent, MemorySubscriptionFilter};
use aibe::ports::outbound::{
    MemoryConfig, MemorySubscriptionBroker, ProfileRegistry, TerminationCapability, ToolsConfig,
};
use aibe_protocol::{
    ClientResponse, MemoryChangeKind, MemoryContext, MemoryInjectPolicyDto, MemoryOperationAdd,
    MemoryOperationDto, MemoryScopeDto, MemoryStatusDto,
};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[test]
fn broker_publishes_to_matching_memory_space() {
    let broker = InProcessMemorySubscriptionBroker::new();
    let mut sub_a = broker.subscribe("space_a".into(), MemorySubscriptionFilter::default());
    let mut sub_b = broker.subscribe("space_b".into(), MemorySubscriptionFilter::default());

    broker.publish(
        "space_a",
        MemoryChangeEvent {
            kind: "goal".into(),
            change: MemoryChangeKind::Added,
            entries: vec![],
        },
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let event_a = rt.block_on(sub_a.recv()).expect("event");
    assert_eq!(event_a.kind, "goal");
    let no_event_b =
        rt.block_on(async { tokio::time::timeout(Duration::from_millis(50), sub_b.recv()).await });
    assert!(no_event_b.is_err() || no_event_b.unwrap().is_none());
}

#[test]
fn broker_kind_filter_excludes_other_kinds() {
    let broker = InProcessMemorySubscriptionBroker::new();
    let mut sub = broker.subscribe(
        "space_a".into(),
        MemorySubscriptionFilter {
            kind: Some("goal".into()),
        },
    );

    broker.publish(
        "space_a",
        MemoryChangeEvent {
            kind: "idea".into(),
            change: MemoryChangeKind::Added,
            entries: vec![],
        },
    );
    broker.publish(
        "space_a",
        MemoryChangeEvent {
            kind: "goal".into(),
            change: MemoryChangeKind::Added,
            entries: vec![],
        },
    );

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let event = rt.block_on(sub.recv()).expect("goal event");
    assert_eq!(event.kind, "goal");
    let no_more =
        rt.block_on(async { tokio::time::timeout(Duration::from_millis(50), sub.recv()).await });
    assert!(no_more.is_err() || no_more.unwrap().is_none());
}

#[test]
fn broker_unregisters_on_drop() {
    let broker = InProcessMemorySubscriptionBroker::new();
    {
        let _sub = broker.subscribe("space_a".into(), MemorySubscriptionFilter::default());
        assert_eq!(broker.subscriber_count(), 1);
    }
    assert_eq!(broker.subscriber_count(), 0);
}

#[test]
fn memory_apply_publishes_event() {
    let dir = tempdir().expect("tempdir");
    let store_impl = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let loader = store_impl.registry_loader();
    let store: Arc<dyn aibe::ports::outbound::ContextualMemoryStore> = Arc::new(store_impl);
    let broker: Arc<dyn MemorySubscriptionBroker> =
        Arc::new(InProcessMemorySubscriptionBroker::new());
    let resolver: Arc<dyn aibe::ports::outbound::MemorySpaceResolver> =
        Arc::new(FilesystemMemorySpaceResolver);
    let service = MemoryService::with_broker(
        Arc::clone(&store),
        Arc::clone(&resolver),
        loader,
        Arc::clone(&broker),
    );
    let mut sub = broker.subscribe("ctx_sub".into(), MemorySubscriptionFilter::default());

    let cwd = std::env::current_dir().expect("cwd");
    let response = service.apply(
        "apply-1".into(),
        "sess-1".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_sub".into()),
        },
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: "rule".into(),
            scope: None,
            inject: None,
            status: None,
            text: "no auto exec".into(),
            make_active: None,
        }),
    );
    assert!(matches!(response, ClientResponse::MemoryApplyResult { .. }));

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let event = rt.block_on(sub.recv()).expect("changed");
    assert_eq!(event.kind, "rule");
    assert_eq!(event.change, MemoryChangeKind::Added);
}

#[tokio::test]
async fn memory_subscribe_receives_apply_over_dedicated_socket() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("mem-sub.sock");
    let llm = Arc::new(aibe::adapters::outbound::MockLlm::new());
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let socket_for_server = socket_path.clone();
    let store_root = dir.path().join("data");
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            profile_registry,
            ToolsConfig::default(),
            Vec::new(),
            "default".to_string(),
            store_root,
            MemoryConfig::default(),
        )
        .await
        .expect("server");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let subscribe_path = socket_path.clone();
    let subscribe = tokio::spawn(async move {
        let stream = UnixStream::connect(&subscribe_path)
            .await
            .expect("connect subscribe");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let cwd = std::env::current_dir().expect("cwd");
        let req = serde_json::json!({
            "type": "memory_subscribe",
            "id": "sub-1",
            "session_id": "sess-sub",
            "context": {
                "cwd": cwd.to_string_lossy(),
                "memory_space_id": "ctx_socket"
            }
        });
        write_line(&mut writer, &req.to_string()).await;
        let result_line = read_line(&mut lines).await;
        assert!(result_line.contains(r#""type":"memory_subscribe_result""#));
        assert!(result_line.contains(r#""memory_space_id":"ctx_socket""#));
        let changed_line = read_line(&mut lines).await;
        changed_line
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    let socket_for_apply = socket_path.clone();
    let apply = tokio::spawn(async move {
        let stream = UnixStream::connect(&socket_for_apply)
            .await
            .expect("connect apply");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let cwd = std::env::current_dir().expect("cwd");
        let req = serde_json::json!({
            "type": "memory_apply",
            "id": "apply-1",
            "session_id": "sess-apply",
            "context": {
                "cwd": cwd.to_string_lossy(),
                "memory_space_id": "ctx_socket"
            },
            "operation": {
                "op": "add",
                "kind": "goal",
                "scope": "project",
                "inject": "pinned",
                "status": "active",
                "text": "ship subscribe",
                "make_active": true
            }
        });
        write_line(&mut writer, &req.to_string()).await;
        let result_line = read_line(&mut lines).await;
        assert!(result_line.contains(r#""type":"memory_apply_result""#));
    });

    let changed_line = subscribe.await.expect("subscribe task");
    apply.await.expect("apply task");
    assert!(changed_line.contains(r#""type":"memory_changed""#));
    assert!(changed_line.contains(r#""kind":"goal""#));
    assert!(changed_line.contains(r#""change":"added""#));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn memory_subscribe_rejects_other_rpc_on_same_connection() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("mem-sub-mix.sock");
    let llm = Arc::new(aibe::adapters::outbound::MockLlm::new());
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let socket_for_server = socket_path.clone();
    let store_root = dir.path().join("data");
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            profile_registry,
            ToolsConfig::default(),
            Vec::new(),
            "default".to_string(),
            store_root,
            MemoryConfig::default(),
        )
        .await
        .expect("server");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let cwd = std::env::current_dir().expect("cwd");
    let subscribe = serde_json::json!({
        "type": "memory_subscribe",
        "id": "sub-1",
        "session_id": "sess-sub",
        "context": {
            "cwd": cwd.to_string_lossy(),
            "memory_space_id": "ctx_mix"
        }
    });
    write_line(&mut writer, &subscribe.to_string()).await;
    let result_line = read_line(&mut lines).await;
    assert!(result_line.contains(r#""type":"memory_subscribe_result""#));

    let ping = serde_json::json!({"type":"ping","id":"p1"});
    write_line(&mut writer, &ping.to_string()).await;
    let err_line = read_line(&mut lines).await;
    assert!(err_line.contains(r#""type":"error""#));
    assert!(err_line.contains("does not accept other requests"));

    server.abort();
    let _ = server.await;
}

async fn write_line<W: AsyncWriteExt + Unpin>(writer: &mut W, line: &str) {
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write");
    writer.flush().await.expect("flush");
}

async fn read_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    lines.next_line().await.expect("read").expect("line")
}
