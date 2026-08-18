//! Poller do clipboard do lab-clip.
//!
//! Desenho copiado do LocalClip oficial (thread própria, ~800 ms, dedup por
//! hash, texto ≥512 KB e imagem >16 MP descartados), estendido pra imagem na
//! onda 4: PNG é codificado AQUI na thread (o oficial faz igual) e chega pronto
//! na UI. A checagem da flag
//! `ExcludeClipboardContentFromMonitorProcessing` é copiada verbatim do
//! oficial — é o que impede senha copiada do LocalKeys de entrar em qualquer
//! histórico.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use history::{ClipItem, ImageItem, Payload};

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

/// RGBA cru da webview/OS → PNG (o que o histórico guarda).
fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let buf = image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(buf)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// Sobe a thread do poller; devolve o canal por onde chegam os itens novos.
pub fn spawn() -> Receiver<ClipItem> {
    let (tx, rx) = std::sync::mpsc::channel::<ClipItem>();
    std::thread::Builder::new()
        .name("clip-poller".into())
        .spawn(move || {
            // Instância própria da thread: arboard/Clipboard não é compartilhável.
            let mut clip = match arboard::Clipboard::new() {
                Ok(c) => c,
                Err(_) => return, // sem clipboard (headless?): thread desiste
            };
            let mut last_text: u64 = 0;
            let mut last_img: u64 = 0;
            loop {
                let skip = SKIP_NEXT.swap(false, Ordering::Relaxed);
                if skip {
                    // Ainda captura os hashes pra não re-inserir na próxima volta.
                    if let Ok(text) = clip.get_text() {
                        last_text = hash_of("text", text.as_bytes());
                    }
                    if let Ok(img) = clip.get_image() {
                        last_img = hash_of("rgba", &img.bytes);
                    }
                } else if !excluded_from_monitoring() {
                    // Texto primeiro (mais comum); imagem se não houver texto.
                    if let Ok(text) = clip.get_text() {
                        if !text.trim().is_empty() && text.len() <= 512 * 1024 {
                            let h = hash_of("text", text.as_bytes());
                            if h != last_text {
                                last_text = h;
                                last_img = 0; // conteúdo novo: zera a outra classe
                                // Receptor morto = app fechou: sair da thread.
                                if tx
                                    .send(ClipItem {
                                        id: 0, // preenchido pelo history::insert
                                        payload: Payload::Text(text),
                                        pinned: false,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            std::thread::sleep(Duration::from_millis(800));
                            continue;
                        }
                    }
                    if let Ok(img) = clip.get_image() {
                        let (w, hgt) = (img.width, img.height);
                        if w > 0 && hgt > 0 && (w as u64 * hgt as u64) <= 16_000_000 {
                            let h = hash_of("rgba", &img.bytes);
                            if h != last_img {
                                last_img = h;
                                last_text = 0;
                                if let Some(png) = encode_png(w, hgt, &img.bytes) {
                                    if tx
                                        .send(ClipItem {
                                            id: 0,
                                            payload: Payload::Image(ImageItem {
                                                png,
                                                w,
                                                h: hgt,
                                            }),
                                            pinned: false,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
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

    #[test]
    fn png_codifica_e_fica_decodificavel() {
        // 2×1 pixels: vermelho e azul.
        let rgba: Vec<u8> = vec![255, 0, 0, 255, 0, 0, 255, 255];
        let png = encode_png(2, 1, &rgba).expect("encode");
        assert!(!png.is_empty());
        assert!(png.starts_with(b"\x89PNG"));
        let dec = image::load_from_memory(&png).expect("decode").to_rgba8();
        assert_eq!(dec.as_raw(), &rgba);
    }
}
