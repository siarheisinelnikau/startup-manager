use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub check_on_startup: bool,
    pub language: crate::i18n::Language,
    pub theme: crate::appearance::Theme,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            language: crate::i18n::Language::default(),
            theme: crate::appearance::Theme::default(),
        }
    }
}

pub fn directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("StartupManager")
}
pub fn load() -> Result<Settings> {
    let path = directory().join("settings.json");
    if !path.exists() {
        return Ok(Settings::default());
    }
    serde_json::from_slice(&std::fs::read(path)?).context("Cannot read settings")
}
pub fn save(settings: &Settings) -> Result<()> {
    write_json(&directory().join("settings.json"), settings)
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("Missing parent directory")?)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_keep_their_preference_and_gain_safe_appearance_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"check_on_startup":false}"#).unwrap();
        assert!(!settings.check_on_startup);
        assert_eq!(settings.language, crate::i18n::Language::English);
        assert_eq!(settings.theme, crate::appearance::Theme::Dark);
        for language in crate::i18n::Language::ALL {
            for theme in [
                crate::appearance::Theme::Dark,
                crate::appearance::Theme::Light,
            ] {
                let settings = Settings {
                    language,
                    theme,
                    check_on_startup: false,
                };
                let restored: Settings =
                    serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
                assert_eq!(restored.language, language);
                assert_eq!(restored.theme, theme);
                assert!(!restored.check_on_startup);
            }
        }
    }
}
