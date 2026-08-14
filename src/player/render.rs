use super::properties::HwDecMode;

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub hwdec: HwDecMode,
    pub video_output: String,
    pub gpu_api: String,
    pub audio_device: String,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            hwdec: HwDecMode::AutoSafe,
            video_output: "gpu".to_string(),
            gpu_api: "auto".to_string(),
            audio_device: "auto".to_string(),
        }
    }
}
