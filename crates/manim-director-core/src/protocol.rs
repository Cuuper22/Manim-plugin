use crate::ProtocolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub request_id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    Event {
        request_id: String,
        event: String,
        #[serde(default)]
        data: Value,
    },
    #[serde(rename = "result")]
    Success {
        request_id: String,
        #[serde(default)]
        result: Value,
    },
    #[serde(rename = "error")]
    Failure {
        request_id: String,
        error: ProtocolError,
    },
}

impl BridgeMessage {
    pub fn id(&self) -> &str {
        match self {
            Self::Event { request_id, .. }
            | Self::Success { request_id, .. }
            | Self::Failure { request_id, .. } => request_id,
        }
    }
}
