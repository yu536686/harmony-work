use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: Value,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IncomingMessage {
    Response(RpcResponse),
    ServerRequest(RpcRequest),
    Notification(RpcNotification),
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid app-server JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("invalid app-server message shape")]
    InvalidShape,
}

pub fn parse_incoming(line: &str) -> Result<IncomingMessage, ProtocolError> {
    let value: Value = serde_json::from_str(line)?;
    let object = value.as_object().ok_or(ProtocolError::InvalidShape)?;
    let id = object.get("id").cloned();
    let method = object.get("method").and_then(Value::as_str);
    let result = object.get("result").cloned();
    let error = object.get("error").cloned();

    match (id, method, result, error) {
        (Some(id), None, result, error) if result.is_some() || error.is_some() => {
            Ok(IncomingMessage::Response(RpcResponse { id, result, error }))
        }
        (Some(id), Some(method), None, None) => {
            Ok(IncomingMessage::ServerRequest(RpcRequest {
                id,
                method: method.to_string(),
                params: object.get("params").cloned().unwrap_or(Value::Null),
            }))
        }
        (None, Some(method), None, None) => {
            Ok(IncomingMessage::Notification(RpcNotification {
                method: method.to_string(),
                params: object.get("params").cloned().unwrap_or(Value::Null),
            }))
        }
        _ => Err(ProtocolError::InvalidShape),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_response_without_jsonrpc_marker() {
        let message = parse_incoming(r#"{"id":7,"result":{"thread":{"id":"thread-1"}}}"#)
            .expect("valid response");

        assert_eq!(
            message,
            IncomingMessage::Response(RpcResponse {
                id: json!(7),
                result: Some(json!({"thread": {"id": "thread-1"}})),
                error: None,
            })
        );
    }

    #[test]
    fn classifies_server_request_for_approval() {
        let message = parse_incoming(
            r#"{"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"command":"npm test"}}"#,
        )
        .expect("valid request");

        assert_eq!(
            message,
            IncomingMessage::ServerRequest(RpcRequest {
                id: json!("approval-1"),
                method: "item/commandExecution/requestApproval".to_string(),
                params: json!({"command": "npm test"}),
            })
        );
    }

    #[test]
    fn classifies_notification() {
        let message = parse_incoming(
            r#"{"method":"item/agentMessage/delta","params":{"delta":"hello"}}"#,
        )
        .expect("valid notification");

        assert_eq!(
            message,
            IncomingMessage::Notification(RpcNotification {
                method: "item/agentMessage/delta".to_string(),
                params: json!({"delta": "hello"}),
            })
        );
    }

    #[test]
    fn rejects_ambiguous_or_malformed_messages() {
        assert!(parse_incoming("not-json").is_err());
        assert!(parse_incoming(r#"{"result":{}}"#).is_err());
        assert!(parse_incoming(r#"{"id":1,"method":"x","result":{}}"#).is_err());
    }
}
