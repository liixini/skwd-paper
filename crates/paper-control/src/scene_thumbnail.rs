use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SceneThumbnailRequest {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SceneThumbnailResponse {
    pub source: String,
    pub error: Option<String>,
}
