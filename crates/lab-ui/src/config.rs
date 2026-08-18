//! Config por app em JSON, no mesmo espírito do padrão Tauri da suíte
//! (theme + lang persistidos), só que resolvido à mão: %APPDATA% no Windows,
//! XDG/~/.config no resto. Load tolerante: ausente ou corrompido = default.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Config {
    pub theme: Theme,
    pub lang: Lang,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Nature,
            lang: Lang::Pt,
        }
    }
}

/// Pasta de config do app (`%APPDATA%\<id>` no Windows, `~/.config/<id>` fora).
pub fn config_dir(app_id: &str) -> PathBuf {
    if let Ok(roaming) = std::env::var("APPDATA") {
        return PathBuf::from(roaming).join(app_id);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join(app_id);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join(app_id)
}

pub fn load(app_id: &str) -> Config {
    load_from(&config_dir(app_id).join("config.json"))
}

pub fn save(app_id: &str, cfg: &Config) -> std::io::Result<()> {
    save_to(&config_dir(app_id).join("config.json"), cfg)
}

pub fn load_from(path: &Path) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let dir = std::env::temp_dir().join(format!(
            "lab-ui-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("config.json");

        let cfg = Config {
            theme: Theme::PunkPrincess,
            lang: Lang::Es,
        };
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), cfg);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ausente_ou_corrompido_vira_default() {
        assert_eq!(load_from(Path::new("/caminho/que/nao/existe.json")), Config::default());
        let dir = std::env::temp_dir().join(format!(
            "lab-ui-test-bad-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("config.json");
        save_to(&path, &Config::default()).unwrap();
        std::fs::write(&path, "{ isso não é json").unwrap();
        assert_eq!(load_from(&path), Config::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
