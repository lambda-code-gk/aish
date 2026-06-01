//! NDJSON request/response transport（同一接続上の承認往復を含む）。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use aibe_protocol::{ClientRequest, ClientResponse};

use crate::unix_connect::connect_unix_stream;

/// `agent_turn` の connect 上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// `shell_exec` 承認 prompt の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecApprovalPrompt {
    pub prompt_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connect to aibe: {0}")]
    Connect(#[from] std::io::Error),
    #[error("serialize request: {0}")]
    Serialize(String),
    #[error("deserialize response: {0}")]
    Deserialize(String),
    #[error("unexpected response: {0}")]
    Unexpected(String),
}

pub fn send_request(stream: &mut UnixStream, request: &ClientRequest) -> std::io::Result<()> {
    let payload = serde_json::to_string(request).map_err(std::io::Error::other)?;
    writeln!(stream, "{payload}")?;
    stream.flush()?;
    Ok(())
}

pub fn read_response_line(stream: &mut UnixStream) -> Result<ClientResponse, ClientError> {
    let mut reader = BufReader::new(stream);
    read_response_from_reader(&mut reader)
}

fn read_response_from_reader<R: BufRead>(reader: &mut R) -> Result<ClientResponse, ClientError> {
    let mut line = String::new();
    reader.read_line(&mut line).map_err(ClientError::Connect)?;
    if line.trim().is_empty() {
        return Err(ClientError::Unexpected("empty response line".into()));
    }
    serde_json::from_str(line.trim()).map_err(|e| ClientError::Deserialize(e.to_string()))
}

/// 接続済み stream 上で `agent_turn` と承認往復を行う（テスト・カスタム接続向け）。
pub fn agent_turn_on_stream(
    stream: UnixStream,
    request: ClientRequest,
    mut on_approval: impl FnMut(ShellExecApprovalPrompt) -> bool,
) -> Result<ClientResponse, ClientError> {
    let mut writer = stream;
    let mut reader = BufReader::new(writer.try_clone().map_err(ClientError::Connect)?);
    send_request(&mut writer, &request).map_err(ClientError::Connect)?;

    loop {
        match read_response_from_reader(&mut reader)? {
            ClientResponse::ShellExecApprovalPrompt {
                id,
                turn_id,
                tool_call_id,
                command,
                args,
            } => {
                let approved = on_approval(ShellExecApprovalPrompt {
                    prompt_id: id.clone(),
                    turn_id: turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    command: command.clone(),
                    args: args.clone(),
                });
                send_request(
                    &mut writer,
                    &ClientRequest::ShellExecApproval {
                        id,
                        turn_id,
                        tool_call_id,
                        approved,
                    },
                )
                .map_err(ClientError::Connect)?;
            }
            final_resp => return Ok(final_resp),
        }
    }
}

/// `agent_turn` を送り、承認 prompt があれば `on_approval` で応答する。
pub fn agent_turn(
    socket_path: &std::path::Path,
    request: ClientRequest,
    on_approval: impl FnMut(ShellExecApprovalPrompt) -> bool,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    agent_turn_on_stream(stream, request, on_approval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_request_serializes_ping() {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        send_request(&mut client, &ClientRequest::Ping { id: "p1".into() }).expect("send");
        let mut line = String::new();
        BufReader::new(&mut server)
            .read_line(&mut line)
            .expect("read");
        assert!(line.contains(r#""type":"ping""#));
    }
}
