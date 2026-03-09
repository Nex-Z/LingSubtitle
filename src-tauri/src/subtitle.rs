use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleEntry {
    pub timestamp: String,
    pub original: String,
    pub translated: Option<String>,
}

pub struct SubtitleManager {
    save_path: PathBuf,
    current_file: Option<PathBuf>,
}

impl SubtitleManager {
    pub fn new(save_path: &str) -> Self {
        let path = PathBuf::from(save_path);
        fs::create_dir_all(&path).ok();
        Self {
            save_path: path,
            current_file: None,
        }
    }

    pub fn start_new_session(&mut self) {
        let now = Local::now();
        let filename = format!("字幕_{}.txt", now.format("%Y-%m-%d_%H-%M-%S"));
        let file_path = self.save_path.join(filename);
        self.current_file = Some(file_path);
    }

    pub fn update_save_path(&mut self, new_path: &str) {
        self.save_path = PathBuf::from(new_path);
        fs::create_dir_all(&self.save_path).ok();
    }

    pub fn save_entry(&self, entry: &SubtitleEntry) -> Result<(), String> {
        let file_path = self
            .current_file
            .as_ref()
            .ok_or("No active session")?;

        fs::create_dir_all(&self.save_path).ok();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .map_err(|e| format!("Failed to open subtitle file: {}", e))?;

        writeln!(file, "[{}] {}", entry.timestamp, entry.original)
            .map_err(|e| format!("Failed to write subtitle: {}", e))?;

        if let Some(ref translated) = entry.translated {
            writeln!(file, "[{}] {}", entry.timestamp, translated)
                .map_err(|e| format!("Failed to write translation: {}", e))?;
        }

        // Add a blank line between entries
        writeln!(file).ok();

        Ok(())
    }

    pub fn get_current_file(&self) -> Option<String> {
        self.current_file
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }
}
