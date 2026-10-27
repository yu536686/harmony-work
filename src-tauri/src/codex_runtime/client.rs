use crate::codex_runtime::protocol::{
    parse_incoming, IncomingMessage, RpcNotification, RpcRequest,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

type PendingResponse = oneshot::Sender<Result<Value, AppServerError>>;

#[derive(Debug, thiserror::Error)]
pub enum AppServerError {
    #[error("app-server transport closed")]
    TransportClosed,
    #[error("app-server transport error: {0}")]
    Transport(String),
    #[error("app-server RPC error: {0}")]
    Rpc(Value),
    #[error("failed to encode app-server request: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("app-server RPC timed out")]
    Timeout,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppServerEvent {
    Notification(RpcNotification),
    ServerRequest(RpcRequest),
    ProtocolError(String),
    TransportError(String),
    Closed,
}

pub struct AppServerEventReceiver {
    receiver: mpsc::UnboundedReceiver<AppServerEvent>,
}

impl AppServerEventReceiver {
    pub async fn recv(&mut self) -> Option<AppServerEvent> {
        self.receiver.recv().await
    }
}

struct ClientInner<W> {
    writer: Mutex<W>,
    pending: StdMutex<HashMap<String, PendingResponse>>,
    next_id: AtomicU64,
}

struct PendingRequest<W> {
    inner: Arc<ClientInner<W>>,
    key: String,
}

impl<W> Drop for PendingRequest<W> {
    fn drop(&mut self) {
        self.inner
            .pending
            .lock()
            .expect("pending responses")
            .remove(&self.key);
    }
}

pub struct AppServerClient<W> {
    inner: Arc<ClientInner<W>>,
}

impl<W> Clone for AppServerClient<W> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<W> AppServerClient<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub fn new<R>(reader: R, writer: W) -> (Self, AppServerEventReceiver)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let inner = Arc::new(ClientInner {
            writer: Mutex::new(writer),
            pending: StdMutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        spawn_reader(reader, Arc::clone(&inner), event_sender);

        (
            Self { inner },
            AppServerEventReceiver {
                receiver: event_receiver,
            },
        )
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        self.request_with_timeout(method, params, Duration::from_secs(30))
            .await
    }

    pub async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AppServerError> {
        tokio::time::timeout(timeout, self.request_inner(method, params))
            .await
            .map_err(|_| AppServerError::Timeout)?
    }

    async fn request_inner(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let key = id.to_string();
        let (sender, receiver) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("pending responses")
            .insert(key.clone(), sender);
        let _pending = PendingRequest {
            inner: self.inner.clone(),
            key,
        };
        let message = json!({"id": id, "method": method, "params": params});
        let client = self.clone();
        // A dropped request must not leave half a JSON frame in the shared pipe.
        tokio::spawn(async move { client.write_message(&message).await })
            .await
            .map_err(|error| AppServerError::Transport(error.to_string()))??;

        receiver
            .await
            .unwrap_or(Err(AppServerError::TransportClosed))
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.write_message(&json!({"method": method, "params": params}))
            .await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), AppServerError> {
        self.write_message(&json!({"id": id, "result": result}))
            .await
    }

    async fn write_message(&self, message: &Value) -> Result<(), AppServerError> {
        let mut bytes = serde_json::to_vec(message)?;
        bytes.push(b'\n');
        let mut writer = self.inner.writer.lock().await;
        writer.write_all(&bytes).await.map_err(map_io_error)?;
        writer.flush().await.map_err(map_io_error)
    }
}

fn spawn_reader<R, W>(
    reader: R,
    inner: Arc<ClientInner<W>>,
    events: mpsc::UnboundedSender<AppServerEvent>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => match parse_incoming(&line) {
                    Ok(IncomingMessage::Response(response)) => {
                        let key = response_key(&response.id);
                        if let Some(sender) = inner
                            .pending
                            .lock()
                            .expect("pending responses")
                            .remove(&key)
                        {
                            let outcome = match response.error {
                                Some(error) => Err(AppServerError::Rpc(error)),
                                None => Ok(response.result.unwrap_or(Value::Null)),
                            };
                            let _ = sender.send(outcome);
                        }
                    }
                    Ok(IncomingMessage::ServerRequest(request)) => {
                        let _ = events.send(AppServerEvent::ServerRequest(request));
                    }
                    Ok(IncomingMessage::Notification(notification)) => {
                        let _ = events.send(AppServerEvent::Notification(notification));
                    }
                    Err(error) => {
                        let _ = events.send(AppServerEvent::ProtocolError(error.to_string()));
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    let _ = events.send(AppServerEvent::TransportError(error.to_string()));
                    break;
                }
            }
        }

        for (_, sender) in inner.pending.lock().expect("pending responses").drain() {
            let _ = sender.send(Err(AppServerError::TransportClosed));
        }
        let _ = events.send(AppServerEvent::Closed);
    });
}

fn response_key(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        _ => id.to_string(),
    }
}

fn map_io_error(error: std::io::Error) -> AppServerError {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::UnexpectedEof => {
            AppServerError::TransportClosed
        }
        _ => AppServerError::Transport(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn cancelled_request_does_not_leave_a_pending_response() {
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let (reader, writer) = split(client_stream);
        let (client, _events) = AppServerClient::new(reader, writer);
        let running_client = client.clone();
        let running =
            tokio::spawn(async move { running_client.request("stalled", json!({})).await });
        let mut lines = BufReader::new(server_stream).lines();
        lines.next_line().await.unwrap().unwrap();
        running.abort();
        let _ = running.await;
        assert!(client.inner.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn bounded_request_expires_and_unregisters_without_a_response() {
        let (client_stream, _server_stream) = tokio::io::duplex(4096);
        let (reader, writer) = split(client_stream);
        let (client, _events) = AppServerClient::new(reader, writer);
        assert!(matches!(
            client
                .request_with_timeout("stalled", json!({}), Duration::from_millis(30))
                .await,
            Err(AppServerError::Timeout)
        ));
        assert!(client.inner.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelling_a_partial_write_preserves_shared_json_framing() {
        use tokio::io::AsyncReadExt;
        let (client_stream, server_stream) = tokio::io::duplex(64);
        let (reader, writer) = split(client_stream);
        let (server_reader, mut server_writer) = split(server_stream);
        let (client, _events) = AppServerClient::new(reader, writer);
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .request("first", json!({"text":"x".repeat(4096)}))
                .await
        });
        let mut reader = BufReader::new(server_reader);
        let mut prefix = [0_u8; 8];
        reader.read_exact(&mut prefix).await.unwrap();
        first.abort();
        let _ = first.await;
        let second_client = client.clone();
        let second = tokio::spawn(async move { second_client.request("second", json!({})).await });
        let mut rest = String::new();
        reader.read_line(&mut rest).await.unwrap();
        let first_json: Value =
            serde_json::from_slice(&[prefix.as_slice(), rest.as_bytes()].concat()).unwrap();
        assert_eq!(first_json["method"], "first");
        rest.clear();
        reader.read_line(&mut rest).await.unwrap();
        let second_json: Value = serde_json::from_str(&rest).unwrap();
        assert_eq!(second_json["method"], "second");
        server_writer
            .write_all(format!("{}\n", json!({"id":second_json["id"],"result":{}})).as_bytes())
            .await
            .unwrap();
        second.await.unwrap().unwrap();
        assert!(client.inner.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn correlates_out_of_order_responses() {
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let (client_reader, client_writer) = split(client_stream);
        let (server_reader, mut server_writer) = split(server_stream);
        let (client, _events) = AppServerClient::new(client_reader, client_writer);

        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .request("thread/start", json!({"cwd": "/tmp/a"}))
                .await
        });
        let second_client = client.clone();
        let second = tokio::spawn(async move {
            second_client
                .request("thread/start", json!({"cwd": "/tmp/b"}))
                .await
        });

        let mut lines = BufReader::new(server_reader).lines();
        let first_line: serde_json::Value =
            serde_json::from_str(&lines.next_line().await.expect("read").expect("first line"))
                .expect("request json");
        let second_line: serde_json::Value =
            serde_json::from_str(&lines.next_line().await.expect("read").expect("second line"))
                .expect("request json");

        let (first_id, second_id) = if first_line["params"]["cwd"] == "/tmp/a" {
            (first_line["id"].clone(), second_line["id"].clone())
        } else {
            (second_line["id"].clone(), first_line["id"].clone())
        };
        server_writer
            .write_all(
                format!(
                    "{}\n{}\n",
                    json!({"id": second_id, "result": {"name": "second"}}),
                    json!({"id": first_id, "result": {"name": "first"}})
                )
                .as_bytes(),
            )
            .await
            .expect("write responses");

        assert_eq!(
            first.await.expect("join").expect("response"),
            json!({"name": "first"})
        );
        assert_eq!(
            second.await.expect("join").expect("response"),
            json!({"name": "second"})
        );
    }

    #[tokio::test]
    async fn routes_notifications_and_server_requests_to_event_stream() {
        let (client_stream, mut server_stream) = tokio::io::duplex(4096);
        let (client_reader, client_writer) = split(client_stream);
        let (_client, mut events) = AppServerClient::new(client_reader, client_writer);

        server_stream
            .write_all(
                concat!(
                    "{\"method\":\"turn/started\",\"params\":{\"turn\":{\"id\":\"t1\"}}}\n",
                    "{\"id\":\"approve-1\",\"method\":\"item/commandExecution/requestApproval\",\"params\":{\"command\":\"npm test\"}}\n"
                )
                .as_bytes(),
            )
            .await
            .expect("write events");

        assert!(matches!(
            events.recv().await,
            Some(AppServerEvent::Notification(notification))
                if notification.method == "turn/started"
        ));
        assert!(matches!(
            events.recv().await,
            Some(AppServerEvent::ServerRequest(request))
                if request.id == json!("approve-1")
        ));
    }

    #[tokio::test]
    async fn fails_pending_request_when_transport_closes() {
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let (client_reader, client_writer) = split(client_stream);
        let (client, _events) = AppServerClient::new(client_reader, client_writer);

        let pending = tokio::spawn(async move { client.request("initialize", json!({})).await });
        drop(server_stream);

        assert!(matches!(
            pending.await.expect("join"),
            Err(AppServerError::TransportClosed)
        ));
    }
}
