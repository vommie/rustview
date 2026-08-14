pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_THUMBNAIL_VERSION: u32 = 1;
pub const HASH_ALGORITHM: &str = "BLAKE3-256";

// LMDB Named Database Identifiers
pub const DB_META: &str = "meta";
pub const DB_SEQUENCES: &str = "sequences";
pub const DB_FOLDERS: &str = "folders";
pub const DB_FOLDERS_BY_PATH: &str = "folders_by_path";
pub const DB_MEDIA: &str = "media";
pub const DB_MEDIA_BY_FOLDER: &str = "media_by_folder";
pub const DB_MEDIA_BY_HASH: &str = "media_by_hash";
pub const DB_TAGS: &str = "tags";
pub const DB_TAGS_BY_PARENT_NAME: &str = "tags_by_parent_name";
pub const DB_TAGS_BY_MEDIA: &str = "tags_by_media";
pub const DB_MEDIA_BY_TAG: &str = "media_by_tag";

// Sequence Key Identifiers
pub const SEQ_FOLDER_ID: &str = "seq_folder_id";
pub const SEQ_MEDIA_ID: &str = "seq_media_id";
pub const SEQ_TAG_ID: &str = "seq_tag_id";
pub const SEQ_REFRESH_GEN: &str = "seq_refresh_gen";
