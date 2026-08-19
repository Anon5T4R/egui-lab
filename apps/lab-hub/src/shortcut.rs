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
pub fn create(
    app_id: &str,
    display: &str,
    exe: &Path,
    icon: &Path,
    r#where: Where,
) -> Result<PathBuf, String> {
    use windows::core::{GUID, Interface, PCWSTR};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, IPersistFile,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KNOWN_FOLDER_FLAG,
        SHGetKnownFolderPath,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    /// O windows-rs 0.58 não exporta `CLSID_ShellLink` — valor documentado
    /// ({00021401-0000-0000-C000-000000000046}, Shell Link object).
    const CLSID_SHELL_LINK: GUID =
        GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe {
        // STA: chamado da thread da UI, init/uninit em par — barato pra um clique.
        let coi = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let result = (|| -> Result<PathBuf, String> {
            let folder_guid = match r#where {
                Where::StartMenu => FOLDERID_Programs,
                Where::Desktop => FOLDERID_Desktop,
            };
            let mut folder = SHGetKnownFolderPath(&folder_guid, KNOWN_FOLDER_FLAG(0), None)
                .map_err(|e| format!("pasta do sistema: {e}"))?;
            let dir = PathBuf::from(folder.to_string().map_err(|e| e.to_string())?);
            CoTaskMemFree(Some(folder.as_ptr().cast()));

            let lnk = dir.join(format!("{display}.lnk"));

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

            let persist: IPersistFile = link.cast().map_err(|e| format!("IPersistFile: {e}"))?;
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
        let _ = app_id;
        result
    }
}

#[cfg(not(windows))]
pub fn create(
    app_id: &str,
    display: &str,
    exe: &Path,
    icon: &Path,
    r#where: Where,
) -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "sem HOME".to_string())?;
    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nName={display}\nExec={}\nIcon={}\nCategories=Utility;\nTerminal=false\n",
        exe.display(),
        icon.display()
    );
    let (dest, content): (PathBuf, String) = match r#where {
        Where::StartMenu => {
            let d = Path::new(&home).join(".local/share/applications");
            (d.join(format!("{app_id}.desktop")), desktop_entry)
        }
        Where::Desktop => {
            let d = Path::new(&home).join("Desktop");
            (d.join(format!("{app_id}.desktop")), desktop_entry)
        }
    };
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&dest, content).map_err(|e| e.to_string())?;
    Ok(dest)
}
