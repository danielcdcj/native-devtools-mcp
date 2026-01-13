use crate::app_protocol::messages::{ProtocolError, ProtocolRequest, ProtocolResponse};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Failed to parse URL: {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("WebSocket connection failed: {0}")]
    ConnectionError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Failed to send message")]
    SendError,

    #[error("Failed to receive response")]
    ReceiveError,

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Protocol error {code}: {message}")]
    Protocol { code: i32, message: String },

    #[error("Not connected to app")]
    NotConnected,
}

impl From<ProtocolError> for ClientError {
    fn from(err: ProtocolError) -> Self {
        ClientError::Protocol {
            code: err.code,
            message: err.message,
        }
    }
}

type PendingRequests = Arc<Mutex<HashMap<u64, oneshot::Sender<ProtocolResponse>>>>;

/// WebSocket client for connecting to app's debug server
#[derive(Clone)]
pub struct AppProtocolClient {
    sender: mpsc::Sender<Message>,
    pending: PendingRequests,
    next_id: Arc<AtomicU64>,
}

impl AppProtocolClient {
    /// Connect to an app's debug server
    pub async fn connect(url_str: &str) -> Result<Self, ClientError> {
        // Validate URL format
        let _url = Url::parse(url_str)?;
        // Pass string directly since tungstenite accepts &str
        let (ws_stream, _) = connect_async(url_str).await?;
        let (mut write, mut read) = ws_stream.split();

        let (tx, mut rx) = mpsc::channel::<Message>(32);
        let pending: PendingRequests = Arc::new(Mutex::new(HashMap::new()));

        // Spawn writer task
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Spawn reader task
        let pending_clone = pending.clone();
        tokio::spawn(async move {
            while let Some(msg_result) = read.next().await {
                if let Ok(Message::Text(text)) = msg_result {
                    if let Ok(response) = serde_json::from_str::<ProtocolResponse>(&text) {
                        let mut pending = pending_clone.lock().await;
                        if let Some(sender) = pending.remove(&response.id) {
                            let _ = sender.send(response);
                        }
                    }
                }
            }
        });

        Ok(Self {
            sender: tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Call a method on the app's debug server
    pub async fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = ProtocolRequest {
            id,
            method: method.to_string(),
            params,
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let msg = Message::Text(serde_json::to_string(&request)?);
        self.sender
            .send(msg)
            .await
            .map_err(|_| ClientError::SendError)?;

        let response = rx.await.map_err(|_| ClientError::ReceiveError)?;

        if let Some(error) = response.error {
            return Err(error.into());
        }

        Ok(response.result.unwrap_or(serde_json::Value::Null))
    }

    // MARK: - Convenience Methods

    /// Get runtime info from the app
    pub async fn get_runtime_info(&self) -> Result<serde_json::Value, ClientError> {
        self.call("Runtime.getInfo", None).await
    }

    /// Get the view tree
    pub async fn get_tree(
        &self,
        depth: Option<i32>,
        root_id: Option<&str>,
    ) -> Result<serde_json::Value, ClientError> {
        let mut params = serde_json::Map::new();
        if let Some(d) = depth {
            params.insert("depth".to_string(), serde_json::Value::Number(d.into()));
        }
        if let Some(id) = root_id {
            params.insert(
                "rootId".to_string(),
                serde_json::Value::String(id.to_string()),
            );
        }
        self.call("View.getTree", Some(serde_json::Value::Object(params)))
            .await
    }

    /// Query for an element
    pub async fn query_selector(&self, selector: &str) -> Result<serde_json::Value, ClientError> {
        self.call(
            "View.querySelector",
            Some(serde_json::json!({ "selector": selector })),
        )
        .await
    }

    /// Query for all matching elements
    pub async fn query_selector_all(
        &self,
        selector: &str,
    ) -> Result<serde_json::Value, ClientError> {
        self.call(
            "View.querySelectorAll",
            Some(serde_json::json!({ "selector": selector })),
        )
        .await
    }

    /// Get element details
    pub async fn get_element(&self, element_id: &str) -> Result<serde_json::Value, ClientError> {
        self.call(
            "View.getElement",
            Some(serde_json::json!({ "elementId": element_id })),
        )
        .await
    }

    /// Take a screenshot
    pub async fn get_screenshot(
        &self,
        element_id: Option<&str>,
    ) -> Result<serde_json::Value, ClientError> {
        let params = element_id.map(|id| serde_json::json!({ "elementId": id }));
        self.call("View.getScreenshot", params).await
    }

    /// Click an element
    pub async fn click(
        &self,
        element_id: &str,
        click_count: Option<i32>,
    ) -> Result<serde_json::Value, ClientError> {
        let mut params = serde_json::json!({ "elementId": element_id });
        if let Some(count) = click_count {
            params["clickCount"] = serde_json::Value::Number(count.into());
        }
        self.call("Input.click", Some(params)).await
    }

    /// Type text
    pub async fn type_text(
        &self,
        text: &str,
        element_id: Option<&str>,
        clear_first: bool,
    ) -> Result<serde_json::Value, ClientError> {
        let mut params = serde_json::json!({
            "text": text,
            "clearFirst": clear_first
        });
        if let Some(id) = element_id {
            params["elementId"] = serde_json::Value::String(id.to_string());
        }
        self.call("Input.type", Some(params)).await
    }

    /// Press a key
    pub async fn press_key(
        &self,
        key: &str,
        modifiers: Vec<String>,
    ) -> Result<serde_json::Value, ClientError> {
        self.call(
            "Input.pressKey",
            Some(serde_json::json!({
                "key": key,
                "modifiers": modifiers
            })),
        )
        .await
    }

    /// Focus an element
    pub async fn focus(&self, element_id: &str) -> Result<serde_json::Value, ClientError> {
        self.call(
            "Input.focus",
            Some(serde_json::json!({ "elementId": element_id })),
        )
        .await
    }

    /// List windows
    pub async fn list_windows(&self) -> Result<serde_json::Value, ClientError> {
        self.call("Window.list", None).await
    }

    /// Focus a window
    pub async fn focus_window(&self, window_id: &str) -> Result<serde_json::Value, ClientError> {
        self.call(
            "Window.focus",
            Some(serde_json::json!({ "windowId": window_id })),
        )
        .await
    }
}
