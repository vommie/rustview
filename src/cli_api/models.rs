use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTagItem {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTagTreeResponse {
    pub success: bool,
    pub total_tags: usize,
    pub tags: Vec<ApiTagItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMediaDetails {
    pub id: u64,
    pub folder_id: u64,
    pub folder_path: String,
    pub filename: String,
    pub full_path: String,
    pub file_size: u64,
    pub modification_time: i64,
    pub rating: u8,
    pub tags: Vec<u32>,
    pub file_hash: String,
    pub width: u32,
    pub height: u32,
    pub media_type: String,
    pub format: String,
    pub is_deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiGetMediaResponse {
    pub success: bool,
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<ApiMediaDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSetMediaResponse {
    pub success: bool,
    pub action: String, // "created" or "updated"
    pub media_id: u64,
    pub folder_id: u64,
    pub filename: String,
    pub rating: u8,
    pub tags: Vec<u32>,
    pub file_hash: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDeleteMediaResponse {
    pub success: bool,
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<u64>,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiHashCheckResponse {
    pub success: bool,
    pub known: bool,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_time: Option<String>,
    pub associated_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiHashInsertResponse {
    pub success: bool,
    pub is_new: bool,
    pub hash: String,
    pub hash_id: u64,
    pub date_time: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub success: bool,
    pub error: String,
}
