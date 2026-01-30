pub mod image_conv;
pub mod audio_conv;
pub mod video_conv;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum ConversionStatus {
    Idle,
    Processing(String),
    #[allow(dead_code)]
    Success(usize),
    #[allow(dead_code)]
    Error(String),
    Completed { success: usize, failures: usize },
}

#[allow(dead_code)]
pub trait Converter {
    fn process(&self, input_files: &[PathBuf], output_folder: &PathBuf, options: &ConversionOptions) -> ConversionStatus;
}

#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub format: String,
    pub quality: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bitrate: String,
    pub codec: String,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            format: String::new(),
            quality: 95,
            width: None,
            height: None,
            bitrate: "256k".to_string(),
            codec: "libx264".to_string(),
        }
    }
}
