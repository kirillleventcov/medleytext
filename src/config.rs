use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct EditorConfig {
    font_size: f32,
}

impl EditorConfig {
    pub const DEFAULT_FONT_SIZE: f32 = 14.0;
    const MIN_FONT_SIZE: f32 = 8.0;
    const MAX_FONT_SIZE: f32 = 72.0;

    pub fn load() -> Self {
        let mut config = Self::default();
        if let Some(path) = Self::config_path() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Some(font_size) = parse_font_size(&contents) {
                    config.font_size = font_size;
                }
            }
        }
        config
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    fn config_path() -> Option<PathBuf> {
        home_dir().map(|home| home.join(".config/medleytext/config"))
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_size: Self::DEFAULT_FONT_SIZE,
        }
    }
}

fn parse_font_size(contents: &str) -> Option<f32> {
    let mut parsed_value = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = match trimmed.split_once('=') {
            Some(pair) => pair,
            None => match trimmed.split_once(':') {
                Some(pair) => pair,
                None => continue,
            },
        };
        if key.trim().eq_ignore_ascii_case("font-size") {
            if let Ok(size) = value.trim().parse::<f32>() {
                let clamped = size.clamp(EditorConfig::MIN_FONT_SIZE, EditorConfig::MAX_FONT_SIZE);
                parsed_value = Some(clamped);
            }
        }
    }
    parsed_value
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOMEDRIVE").and_then(|drive| {
                std::env::var_os("HOMEPATH").map(|path| Path::new(&drive).join(path))
            })
        })
}
