use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings { pub check_on_startup: bool }
impl Default for Settings { fn default() -> Self { Self { check_on_startup: true } } }

pub fn directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("StartupManager")
}
pub fn load() -> Result<Settings> {
    let path = directory().join("settings.json");
    if !path.exists() { return Ok(Settings::default()); }
    serde_json::from_slice(&std::fs::read(path)?).context("Cannot read settings")
}
pub fn save(settings: &Settings) -> Result<()> { write_json(&directory().join("settings.json"), settings) }
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("Missing parent directory")?)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| { use std::io::Write; let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?; file.write_all(&serde_json::to_vec_pretty(value)?)?; file.sync_all()?; drop(file); std::fs::rename(&temporary, path)?; Ok(()) })();
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}
