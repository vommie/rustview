use super::properties::HwDecMode;
use crate::settings::{
    DEFAULT_AUDIO_DEVICE, DEFAULT_GPU_API, DEFAULT_HWDEC_MODE, DEFAULT_VIDEO_OUTPUT,
};

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
            hwdec: HwDecMode::from_str_name(DEFAULT_HWDEC_MODE),
            video_output: DEFAULT_VIDEO_OUTPUT.to_string(),
            gpu_api: DEFAULT_GPU_API.to_string(),
            audio_device: DEFAULT_AUDIO_DEVICE.to_string(),
        }
    }
}
