//! Atalhos com o ícone REAL do app (baixado do irmão Tauri na instalação).
//!
//! Windows: `.lnk` de verdade via COM (`IShellLinkW` + `IPersistFile`) —
//! nada de script/batch. Pastas via `SHGetKnownFolderPath` (Desktop do
//! OneDrive-redirect incluído).
//! Linux: arquivo `.desktop` (menu) e cópia no Desktop.

use std::path::{Path, PathBuf};

pub enum Where {
    StartMenu,
    Desktop,
}

#[cfg(windows)]
mod win {
    use super::Where;
    use std::path::PathBuf;
    use windows::core::{GUID, Interface, PCWSTR};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, IPersistFile, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG,
    };
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// O windows-rs 0.58 não exporta `CLSID_ShellLink` — valor documentado
    /// ({00021401-0000-0000-C000-000000000046}, Shell Link object).
    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Pasta de destino do atalho (Start Menu ou Desktop do USUÁRIO — o do
    /// OneDrive-redirect incluso, porque é a known folder do shell).
    unsafe fn folder_path(r#where: &Where) -> Result<PathBuf, String> {
        let folder_guid = match r#where {
            Where::StartMenu => FOLDERID_Programs,
            Where::Desktop => FOLDERID_Desktop,
        };
        let mut folder =
            SHGetKnownFolderPath(&folder_guid, KNOWN_FOLDER_FLAG(0), None)
                .map_err(|e| format!("pasta do sistema: {e}"))?;
        let dir = PathBuf::from(folder.to_string().map_err(|e| e.to_string())?);
        CoTaskMemFree(Some(folder.as_ptr().cast()));
        Ok(dir)
    }

    /// Caminho do .lnk desta (app, pasta).
    unsafe fn lnk_path(display: &str, r#where: &Where) -> Result<PathBuf, String> {
        Ok(folder_path(r#where)?.join(format!("{display}.lnk")))
    }

    pub fn create(
        exe: &Path,
        icon: &Path,
        display: &str,
        r#where: Where,
    ) -> Result<PathBuf, String> {
        unsafe {
            // STA: chamado da thread da UI, init/uninit em par — barato pra um clique.
            let coi = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let result = (|| -> Result<PathBuf, String> {
                let lnk = lnk_path(display, &r#where)?;

                let link: IShellLinkW =
                    CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("ShellLink: {e}"))?;
                link.SetPath(PCWSTR::from_raw(wide(&exe.display().to_string()).as_ptr()))
                    .map_err(|e| format!("SetPath: {e}"))?;
                if let Some(dir) = exe.parent() {
                    let _ = link.SetWorkingDirectory(PCWSTR::from_raw(
                        wide(&dir.display().to_string()).as_ptr(),
                    ));
                }
                // O ícone real: aponta pro .ico baixado (vindo do irmão Tauri).
                if icon.exists() {
                    let _ = link.SetIconLocation(
                        PCWSTR::from_raw(wide(&icon.display().to_string()).as_ptr()),
                        0,
                    );
                }
                let _ = link.SetDescription(PCWSTR::from_raw(wide(display).as_ptr()));
                let _ = link.SetShowCmd(SW_SHOWNORMAL);

                let persist: IPersistFile =
                    link.cast().map_err(|e| format!("IPersistFile: {e}"))?;
                persist
                    .Save(
                        PCWSTR::from_raw(wide(&lnk.display().to_string()).as_ptr()),
                        BOOL::from(true),
                    )
                    .map_err(|e| format!("Save: {e}"))?;
                Ok(lnk)
            })();

            if coi.is_ok() {
                CoUninitialize();
            }
            result
        }
    }

    /// Remove o atalho criado pelo `create` (usado no uninstall).
    /// Idempotente: atalho inexistente não é erro.
    pub fn remove(display: &str, r#where: Where) -> Result<(), String> {
        unsafe {
            let coi = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let result = (|| -> Result<(), String> {
                let lnk = lnk_path(display, &r#where)?;
                std::fs::remove_file(&lnk).ok();
                Ok(())
            })();
            if coi.is_ok() {
                CoUninitialize();
            }
            result
        }
    }
}

#[cfg(not(windows))]
mod nix {
    use super::Where;
    use std::path::{Path, PathBuf};

    fn desktop_path(app_id: &str, r#where: &Where) -> Result<PathBuf, String> {
        let home = std::env::var("HOME").map_err(|_| "sem HOME".to_string())?;
        Ok(match r#where {
            Where::StartMenu => Path::new(&home)
                .join(".local/share/applications")
                .join(format!("{app_id}.desktop")),
            Where::Desktop => Path::new(&home)
                .join("Desktop")
                .join(format!("{app_id}.desktop")),
        })
    }

    pub fn create(
        app_id: &str,
        exe: &Path,
        icon: &Path,
        display: &str,
        r#where: Where,
    ) -> Result<PathBuf, String> {
        let dest = desktop_path(app_id, &r#where)?;
        let content = format!(
            "[Desktop Entry]\nType=Application\nName={display}\nExec={}\nIcon={}\nCategories=Utility;\nTerminal=false\n",
            exe.display(),
            icon.display()
        );
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, content).map_err(|e| e.to_string())?;
        Ok(dest)
    }

    pub fn remove(app_id: &str, r#where: Where) -> Result<(), String> {
        let dest = desktop_path(app_id, &r#where)?;
        std::fs::remove_file(&dest).ok();
        Ok(())
    }
}

/// Cria o atalho com ícone; devolve o caminho do atalho criado.
#[cfg(windows)]
pub fn create(_app_id: &str, display: &str, exe: &Path, icon: &Path, r#where: Where) -> Result<PathBuf, String> {
    win::create(exe, icon, display, r#where)
}

#[cfg(not(windows))]
pub fn create(app_id: &str, display: &str, exe: &Path, icon: &Path, r#where: Where) -> Result<PathBuf, String> {
    nix::create(app_id, exe, icon, display, r#where)
}

/// Remove o atalho (uninstall). Idempotente. Windows nomeia o .lnk pelo
/// display; Linux nomeia o .desktop pelo id — recebe os dois.
#[cfg(windows)]
pub fn remove(_app_id: &str, display: &str, r#where: Where) -> Result<(), String> {
    win::remove(display, r#where)
}

#[cfg(not(windows))]
pub fn remove(app_id: &str, _display: &str, r#where: Where) -> Result<(), String> {
    nix::remove(app_id, r#where)
}
