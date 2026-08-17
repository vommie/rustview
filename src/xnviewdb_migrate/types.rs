#[derive(Debug, Clone)]
pub struct XnViewFolder {
    pub folder_id: i64,
    pub pathname: String,
}

#[derive(Debug, Clone)]
pub struct XnViewTag {
    pub tag_id: i64,
    pub label: String,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct XnViewHash {
    pub hash_id: i64,
    pub hash: String,
    pub date_time: String,
}

#[derive(Debug, Clone)]
pub struct XnViewImage {
    pub image_id: i64,
    pub folder_id: i64,
    pub filename: String,
    pub size: i64,
    pub modified_date: i64,
    pub rating: i64,
    pub color: i64,
    pub hash_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct XnViewImageInfo {
    pub image_id: i64,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub duration: u64,
}

#[derive(Debug, Clone)]
pub struct XnViewTagLink {
    pub image_id: i64,
    pub tag_id: i64,
}

/// Consolidated container holding raw extracted records from the XnViewMP database.
#[derive(Debug, Default)]
pub struct XnViewRawData {
    pub version: Option<String>,
    pub sub_version: Option<i64>,
    pub folders: Vec<XnViewFolder>,
    pub tags: Vec<XnViewTag>,
    pub hashes: Vec<XnViewHash>,
    pub hashes_table_found: bool,
    pub images: Vec<XnViewImage>,
    pub image_info: std::collections::HashMap<i64, XnViewImageInfo>,
    pub tag_links: Vec<XnViewTagLink>,
}
