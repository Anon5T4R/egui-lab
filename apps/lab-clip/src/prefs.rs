//! Preferências do lab-clip além de tema/idioma (que já vão no config do
//! lab-ui): atalho global **configurável** (com captura) e **iniciar com o
//! sistema**. O atalho persiste em `prefs.json`; o autostart persiste no SO
//! (registro Run no Windows / .desktop no Linux) — o SO é a fonte da verdade,
//! como no LocalClip oficial ("a intenção mora no banco, o registro é efeito").

use std::path::PathBuf;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};

const PREFS_FILE: &str = "prefs.json";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HotkeyPref {
    pub mods: Modifiers,
    pub code: Code,
}

impl Default for HotkeyPref {
    fn default() -> Self {
        // Mesmo default de sempre: Ctrl+Alt+V (o oficial é dono do Ctrl+Shift+V).
        Self {
            mods: Modifiers::CONTROL | Modifiers::ALT,
            code: Code::KeyV,
        }
    }
}

impl HotkeyPref {
    pub fn hotkey(&self) -> HotKey {
        HotKey::new(Some(self.mods), self.code)
    }

    /// "Ctrl+Alt+V" — legível, pro selo da UI e pra depuração.
    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods.contains(Modifiers::CONTROL) {
            parts.push("Ctrl".into());
        }
        if self.mods.contains(Modifiers::SUPER) {
            parts.push("Win".into());
        }
        if self.mods.contains(Modifiers::ALT) {
            parts.push("Alt".into());
        }
        if self.mods.contains(Modifiers::SHIFT) {
            parts.push("Shift".into());
        }
        parts.push(code_display(self.code));
        parts.join("+")
    }
}

fn code_display(c: Code) -> String {
    match c {
        Code::KeyA => "A".into(),
        Code::KeyB => "B".into(),
        Code::KeyC => "C".into(),
        Code::KeyD => "D".into(),
        Code::KeyE => "E".into(),
        Code::KeyF => "F".into(),
        Code::KeyG => "G".into(),
        Code::KeyH => "H".into(),
        Code::KeyI => "I".into(),
        Code::KeyJ => "J".into(),
        Code::KeyK => "K".into(),
        Code::KeyL => "L".into(),
        Code::KeyM => "M".into(),
        Code::KeyN => "N".into(),
        Code::KeyO => "O".into(),
        Code::KeyP => "P".into(),
        Code::KeyQ => "Q".into(),
        Code::KeyR => "R".into(),
        Code::KeyS => "S".into(),
        Code::KeyT => "T".into(),
        Code::KeyU => "U".into(),
        Code::KeyV => "V".into(),
        Code::KeyW => "W".into(),
        Code::KeyX => "X".into(),
        Code::KeyY => "Y".into(),
        Code::KeyZ => "Z".into(),
        Code::Digit0 => "0".into(),
        Code::Digit1 => "1".into(),
        Code::Digit2 => "2".into(),
        Code::Digit3 => "3".into(),
        Code::Digit4 => "4".into(),
        Code::Digit5 => "5".into(),
        Code::Digit6 => "6".into(),
        Code::Digit7 => "7".into(),
        Code::Digit8 => "8".into(),
        Code::Digit9 => "9".into(),
        Code::F1 => "F1".into(),
        Code::F2 => "F2".into(),
        Code::F3 => "F3".into(),
        Code::F4 => "F4".into(),
        Code::F5 => "F5".into(),
        Code::F6 => "F6".into(),
        Code::F7 => "F7".into(),
        Code::F8 => "F8".into(),
        Code::F9 => "F9".into(),
        Code::F10 => "F10".into(),
        Code::F11 => "F11".into(),
        Code::F12 => "F12".into(),
        Code::Space => "Space".into(),
        Code::Enter => "Enter".into(),
        Code::Escape => "Esc".into(),
        Code::Tab => "Tab".into(),
        Code::Backspace => "Backspace".into(),
        Code::Delete => "Delete".into(),
        Code::Home => "Home".into(),
        Code::End => "End".into(),
        Code::PageUp => "PageUp".into(),
        Code::PageDown => "PageDown".into(),
        Code::ArrowLeft => "←".into(),
        Code::ArrowRight => "→".into(),
        Code::ArrowUp => "↑".into(),
        Code::ArrowDown => "↓".into(),
        Code::Minus => "-".into(),
        Code::Equal => "=".into(),
        Code::Comma => ",".into(),
        Code::Period => ".".into(),
        Code::Slash => "/".into(),
        Code::Semicolon => ";".into(),
        Code::Quote => "'".into(),
        Code::Backslash => "\\".into(),
        Code::BracketLeft => "[".into(),
        Code::BracketRight => "]".into(),
        Code::Backquote => "`".into(),
        _ => format!("{c:?}"),
    }
}

// ── persistência ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct PrefsFile {
    /// Bits do Modifiers (bitflags do keyboard_types).
    hotkey_mods: u32,
    /// Nome Debug do Code ("KeyV", "Digit1", "F5"…).
    hotkey_code: String,
}

impl Default for PrefsFile {
    fn default() -> Self {
        let d = HotkeyPref::default();
        Self {
            hotkey_mods: d.mods.bits(),
            hotkey_code: format!("{:?}", d.code),
        }
    }
}

fn prefs_path() -> PathBuf {
    lab_ui::config::config_dir(super::APP_ID).join(PREFS_FILE)
}

pub fn load() -> HotkeyPref {
    let Ok(s) = std::fs::read_to_string(prefs_path()) else {
        return HotkeyPref::default();
    };
    let Ok(pf) = serde_json::from_str::<PrefsFile>(&s) else {
        return HotkeyPref::default();
    };
    let Some(mods) = Modifiers::from_bits(pf.hotkey_mods) else {
        return HotkeyPref::default();
    };
    let Some(code) = code_from_name(&pf.hotkey_code) else {
        return HotkeyPref::default();
    };
    HotkeyPref { mods, code }
}

pub fn save(hk: &HotkeyPref) {
    let pf = PrefsFile {
        hotkey_mods: hk.mods.bits(),
        hotkey_code: format!("{:?}", hk.code),
    };
    let p = prefs_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&pf) {
        let _ = std::fs::write(p, json);
    }
}

fn code_from_name(name: &str) -> Option<Code> {
    Some(match name {
        "KeyA" => Code::KeyA,
        "KeyB" => Code::KeyB,
        "KeyC" => Code::KeyC,
        "KeyD" => Code::KeyD,
        "KeyE" => Code::KeyE,
        "KeyF" => Code::KeyF,
        "KeyG" => Code::KeyG,
        "KeyH" => Code::KeyH,
        "KeyI" => Code::KeyI,
        "KeyJ" => Code::KeyJ,
        "KeyK" => Code::KeyK,
        "KeyL" => Code::KeyL,
        "KeyM" => Code::KeyM,
        "KeyN" => Code::KeyN,
        "KeyO" => Code::KeyO,
        "KeyP" => Code::KeyP,
        "KeyQ" => Code::KeyQ,
        "KeyR" => Code::KeyR,
        "KeyS" => Code::KeyS,
        "KeyT" => Code::KeyT,
        "KeyU" => Code::KeyU,
        "KeyV" => Code::KeyV,
        "KeyW" => Code::KeyW,
        "KeyX" => Code::KeyX,
        "KeyY" => Code::KeyY,
        "KeyZ" => Code::KeyZ,
        "Digit0" => Code::Digit0,
        "Digit1" => Code::Digit1,
        "Digit2" => Code::Digit2,
        "Digit3" => Code::Digit3,
        "Digit4" => Code::Digit4,
        "Digit5" => Code::Digit5,
        "Digit6" => Code::Digit6,
        "Digit7" => Code::Digit7,
        "Digit8" => Code::Digit8,
        "Digit9" => Code::Digit9,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "Space" => Code::Space,
        "Enter" => Code::Enter,
        "Escape" => Code::Escape,
        "Tab" => Code::Tab,
        "Backspace" => Code::Backspace,
        "Delete" => Code::Delete,
        "Home" => Code::Home,
        "End" => Code::End,
        "PageUp" => Code::PageUp,
        "PageDown" => Code::PageDown,
        "ArrowLeft" => Code::ArrowLeft,
        "ArrowRight" => Code::ArrowRight,
        "ArrowUp" => Code::ArrowUp,
        "ArrowDown" => Code::ArrowDown,
        "Minus" => Code::Minus,
        "Equal" => Code::Equal,
        "Comma" => Code::Comma,
        "Period" => Code::Period,
        "Slash" => Code::Slash,
        "Semicolon" => Code::Semicolon,
        "Quote" => Code::Quote,
        "Backslash" => Code::Backslash,
        "BracketLeft" => Code::BracketLeft,
        "BracketRight" => Code::BracketRight,
        "Backquote" => Code::Backquote,
        _ => return None,
    })
}

/// Converte o teclado do egui pro Code do global-hotkey (teclas modificadoras
/// NÃO são egui::Key — entram pelos Modifiers do frame).
pub fn egui_key_to_code(key: egui::Key) -> Option<Code> {
    use egui::Key as K;
    Some(match key {
        K::A => Code::KeyA,
        K::B => Code::KeyB,
        K::C => Code::KeyC,
        K::D => Code::KeyD,
        K::E => Code::KeyE,
        K::F => Code::KeyF,
        K::G => Code::KeyG,
        K::H => Code::KeyH,
        K::I => Code::KeyI,
        K::J => Code::KeyJ,
        K::K => Code::KeyK,
        K::L => Code::KeyL,
        K::M => Code::KeyM,
        K::N => Code::KeyN,
        K::O => Code::KeyO,
        K::P => Code::KeyP,
        K::Q => Code::KeyQ,
        K::R => Code::KeyR,
        K::S => Code::KeyS,
        K::T => Code::KeyT,
        K::U => Code::KeyU,
        K::V => Code::KeyV,
        K::W => Code::KeyW,
        K::X => Code::KeyX,
        K::Y => Code::KeyY,
        K::Z => Code::KeyZ,
        K::Num0 => Code::Digit0,
        K::Num1 => Code::Digit1,
        K::Num2 => Code::Digit2,
        K::Num3 => Code::Digit3,
        K::Num4 => Code::Digit4,
        K::Num5 => Code::Digit5,
        K::Num6 => Code::Digit6,
        K::Num7 => Code::Digit7,
        K::Num8 => Code::Digit8,
        K::Num9 => Code::Digit9,
        K::F1 => Code::F1,
        K::F2 => Code::F2,
        K::F3 => Code::F3,
        K::F4 => Code::F4,
        K::F5 => Code::F5,
        K::F6 => Code::F6,
        K::F7 => Code::F7,
        K::F8 => Code::F8,
        K::F9 => Code::F9,
        K::F10 => Code::F10,
        K::F11 => Code::F11,
        K::F12 => Code::F12,
        K::Space => Code::Space,
        K::Enter => Code::Enter,
        K::Escape => Code::Escape,
        K::Tab => Code::Tab,
        K::Backspace => Code::Backspace,
        K::Delete => Code::Delete,
        K::Home => Code::Home,
        K::End => Code::End,
        K::PageUp => Code::PageUp,
        K::PageDown => Code::PageDown,
        K::ArrowLeft => Code::ArrowLeft,
        K::ArrowRight => Code::ArrowRight,
        K::ArrowUp => Code::ArrowUp,
        K::ArrowDown => Code::ArrowDown,
        K::Minus => Code::Minus,
        K::Equals => Code::Equal,
        K::Comma => Code::Comma,
        K::Period => Code::Period,
        K::Slash => Code::Slash,
        K::Semicolon => Code::Semicolon,
        K::Quote => Code::Quote,
        K::Backslash => Code::Backslash,
        K::OpenBracket => Code::BracketLeft,
        K::CloseBracket => Code::BracketRight,
        K::Backtick => Code::Backquote,
        _ => return None,
    })
}

pub fn egui_mods_to_gh(m: egui::Modifiers) -> Modifiers {
    let mut out = Modifiers::empty();
    if m.ctrl {
        out |= Modifiers::CONTROL;
    }
    if m.alt {
        out |= Modifiers::ALT;
    }
    if m.shift {
        out |= Modifiers::SHIFT;
    }
    if m.command || m.mac_cmd {
        out |= Modifiers::SUPER;
    }
    out
}

// ── autostart (o SO é a fonte da verdade) ─────────────────────────────

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>("LabClip"))
        .is_ok()
}

#[cfg(windows)]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if enabled {
        key.set_value("LabClip", &format!("\"{}\" --hidden", exe.display()))
            .map_err(|e| e.to_string())
    } else {
        key.delete_value("LabClip").map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
fn autostart_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config/autostart")
        .join("lab-clip.desktop")
}

#[cfg(not(windows))]
pub fn autostart_enabled() -> bool {
    autostart_path().exists()
}

#[cfg(not(windows))]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let p = autostart_path();
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            &p,
            format!(
                "[Desktop Entry]\nType=Application\nName=Lab Clip\nExec=\"{}\" --hidden\nTerminal=false\n",
                exe.display()
            ),
        )
        .map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(&p).map_err(|e| e.to_string())
    }
}
