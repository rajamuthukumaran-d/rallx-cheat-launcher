#![allow(dead_code)]

use std::path::Path;

#[derive(Debug, Clone)]
pub struct TrainerInfo {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
}

pub fn discover_trainers(folder: &Path) -> Result<Vec<TrainerInfo>, std::io::Error> {
    let mut trainers = Vec::new();
    if folder.is_dir() {
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "exe") {
                if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                    let metadata = entry.metadata()?;
                    trainers.push(TrainerInfo {
                        name: filename.to_string(),
                        filename: filename.to_string(),
                        size_bytes: metadata.len(),
                    });
                }
            }
        }
    }
    Ok(trainers)
}

pub fn launch_trainer(folder: &Path, filename: &str) -> Result<(), std::io::Error> {
    let full_path = folder.join(filename);
    std::process::Command::new(full_path).spawn()?;
    Ok(())
}
