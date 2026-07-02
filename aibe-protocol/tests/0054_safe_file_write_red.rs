// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use aibe_protocol::{
    sanitize_readonly_advisory_tools, ClientRequest, ClientResponse, ToolApprovalOrigin, ToolName,
    ToolRiskClass, APPLY_PATCH, WRITE_FILE,
};

#[test]
fn sanitize_readonly_advisory_tools_excludes_write_tools() {
    let got = sanitize_readonly_advisory_tools(&[
        "read_file".into(),
        WRITE_FILE.into(),
        APPLY_PATCH.into(),
        "grep".into(),
    ]);
    assert_eq!(got, vec!["read_file".to_string(), "grep".to_string()]);
}

#[test]
fn tool_approval_prompt_roundtrip() {
    let prompt = ClientResponse::ToolApprovalPrompt {
        id: "p1".into(),
        turn_id: "t1".into(),
        tool_call_id: "c1".into(),
        tool_name: WRITE_FILE.into(),
        risk_class: ToolRiskClass::WriteLike,
        summary: "replace src/main.rs (+3/-1)".into(),
        paths: vec!["src/main.rs".into()],
        preview: "@@ -1,3 +1,4 @@\n old\n+new\n".into(),
        preview_truncated: false,
    };
    let json = serde_json::to_string(&prompt).expect("serialize");
    assert!(json.contains(r#""type":"tool_approval_prompt""#));
    assert!(json.contains(r#""risk_class":"write_like""#));
    let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(
        back,
        ClientResponse::ToolApprovalPrompt {
            tool_name,
            risk_class: ToolRiskClass::WriteLike,
            preview_truncated: false,
            ..
        } if tool_name == WRITE_FILE
    ));
}

#[test]
fn tool_approval_request_roundtrip() {
    let req = ClientRequest::ToolApproval {
        id: "p1".into(),
        turn_id: "t1".into(),
        tool_call_id: "c1".into(),
        approved: true,
        approval_origin: ToolApprovalOrigin::UiYes,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    assert!(json.contains(r#""type":"tool_approval""#));
    assert!(json.contains(r#""approval_origin":"ui_yes""#));
    let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(
        back,
        ClientRequest::ToolApproval {
            approved: true,
            approval_origin: ToolApprovalOrigin::UiYes,
            ..
        }
    ));
}

#[test]
fn write_tools_are_known_tool_names() {
    assert_eq!("write_file".parse(), Ok(ToolName::write_file()));
    assert_eq!("apply_patch".parse(), Ok(ToolName::apply_patch()));
}
