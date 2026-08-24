//! Copiar segredo sem vazar em histórico/nuvem.
//!
//! Windows: copiado verbatim do LocalKeys (`src-tauri/src/clipboard.rs`) —
//! escreve marcando os formatos de exclusão (KeePass/1Password-style) e limpa
//! após 30 s, só se o conteúdo ainda for o nosso. É o que impede a senha de
//! cair no Win+V e no histórico do lab-clip/LocalClip.
//!
//! Fora do Windows: arboard + a mesma limpeza de 30 s (sem os formatos, que
//! são convenção do Windows).

#[cfg(windows)]
mod imp {
    //! Clipboard no Windows com **exclusão do histórico (Win+V) e da nuvem**.
    use std::time::Duration;

    use clipboard_win::{
        empty, get_clipboard_string, raw, register_format, with_clipboard, Clipboard,
    };

    /// Formatos que instruem o Windows a não guardar/sincronizar o conteúdo.
    const EXCLUSION_FORMATS: [&str; 3] = [
        "ExcludeClipboardContentFromMonitorProcessing",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
    ];

    const CLEAR_AFTER: Duration = Duration::from_secs(30);

    pub fn copy_secret(text: String) -> Result<(), String> {
        {
            // Abre a área de transferência (RAII: fecha ao sair do bloco).
            let _clip = Clipboard::new_attempts(10).map_err(|e| format!("abrir clipboard: {e}"))?;
            // set_string limpa e grava o texto (CF_UNICODETEXT).
            raw::set_string(&text).map_err(|e| format!("gravar clipboard: {e}"))?;
            // Adiciona os formatos de exclusão SEM limpar o texto.
            for name in EXCLUSION_FORMATS {
                if let Some(fmt) = register_format(name) {
                    let _ = raw::set_without_clear(fmt.get(), &[0u8; 4]);
                }
            }
        }

        // Limpa em 30 s, mas só se a área de transferência ainda contiver o segredo
        // (para não apagar algo que o usuário copiou depois).
        std::thread::spawn(move || {
            std::thread::sleep(CLEAR_AFTER);
            if let Ok(current) = get_clipboard_string() {
                if current == text {
                    let _ = with_clipboard(|| {
                        let _ = empty();
                    });
                }
            }
        });
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::time::Duration;

    const CLEAR_AFTER: Duration = Duration::from_secs(30);

    pub fn copy_secret(text: String) -> Result<(), String> {
        let mut c = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        c.set_text(text.clone()).map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            std::thread::sleep(CLEAR_AFTER);
            if let Ok(mut c) = arboard::Clipboard::new() {
                let ainda_nosso = c.get_text().map(|t| t == text).unwrap_or(false);
                if ainda_nosso {
                    let _ = c.set_text("");
                }
            }
        });
        Ok(())
    }
}

pub use imp::copy_secret;
