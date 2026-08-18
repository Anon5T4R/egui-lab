//! Poller do clipboard do lab-clip.
//!
//! Desenho copiado do LocalClip oficial (thread própria, ~800 ms, dedup por
//! hash, item ≥512 KB descartado) com duas diferenças de escopo: só TEXTO
//! (imagem fica fora do lab) e histórico em memória (o oficial usa SQLite).
//! A checagem da flag `ExcludeClipboardContentFromMonitorProcessing` é copiada
//! verbatim do oficial — é o que impede senha copiada do LocalKeys de entrar
//! em qualquer histórico.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// O copy disparado por NÓS (re-copiar item) não deve voltar pro histórico.
pub static SKIP_NEXT: AtomicBool = AtomicBool::new(false);

fn hash_of(kind: &str, data: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    (kind, data).hash(&mut h);
    h.finish()
}

/// Windows: o conteúdo atual pediu pra ficar FORA de históricos?
/// Copiado verbatim do LocalClip oficial — `IsClipboardFormatAvailable` não
/// exige abrir o clipboard, é checagem barata.
#[cfg(windows)]
fn excluded_from_monitoring() -> bool {
    use std::sync::OnceLock;
    static FMT: OnceLock<u32> = OnceLock::new();
    let fmt = *FMT.get_or_init(|| {
        clipboard_win::register_format("ExcludeClipboardContentFromMonitorProcessing")
            .map(|f| f.get())
            .unwrap_or(0)
    });
    fmt != 0 && clipboard_win::is_format_avail(fmt)
}

#[cfg(not(windows))]
fn excluded_from_monitoring() -> bool {
    false
}

/// Sobe a thread do poller; devolve o canal por onde chegam os textos novos.
pub fn spawn() -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("clip-poller".into())
        .spawn(move || {
            // Instância própria da thread: arboard/Clipboard não é compartilhável.
            let mut clip = match arboard::Clipboard::new() {
                Ok(c) => c,
                Err(_) => return, // sem clipboard (headless?): thread desiste
            };
            let mut last: u64 = 0;
            loop {
                if SKIP_NEXT.swap(false, Ordering::Relaxed) {
                    // Ainda captura o hash pra não re-inserir na próxima volta.
                    if let Ok(text) = clip.get_text() {
                        last = hash_of("text", text.as_bytes());
                    }
                } else if !excluded_from_monitoring() {
                    if let Ok(text) = clip.get_text() {
                        if !text.trim().is_empty() && text.len() <= 512 * 1024 {
                            let h = hash_of("text", text.as_bytes());
                            if h != last {
                                last = h;
                                // Receptor morto = app fechou: sair da thread.
                                if tx.send(text).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(800));
            }
        })
        .expect("spawn clip-poller");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_estavel_e_distinto() {
        assert_eq!(hash_of("text", b"a"), hash_of("text", b"a"));
        assert_ne!(hash_of("text", b"a"), hash_of("text", b"b"));
    }
}
