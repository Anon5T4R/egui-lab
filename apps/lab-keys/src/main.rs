//! lab-keys — piloto de referência do LocalKeys em egui/eframe.
//! Onda 3: cofre `.tkeys` REAL (`crypto.rs`/`totp.rs`/gravação atômica/
//! `copy_secret` copiados verbatim do LocalKeys@0.9.0).
//! Onda 4: TOTP ao vivo (código + contagem), **desbloqueio rápido** (chave
//! derivada no cofre do SO via keyring — mesmo desenho do oficial, opt-in),
//! **editar** item e **excluir** (lixeira lógica, `deletedAt`).
//! Onda 5: **bandeja** (Windows): o X esconde pra bandeja; "Sair" no menu
//! fecha de verdade. Os handlers do tray-icon vivem numa thread própria
//! (janela oculta congela o update — ver `winctl.rs`), como no lab-clip.
//! Onda 6: paridade de FEATURES com o LocalKeys oficial — importação (KeePass
//! `.kdbx`, Bitwarden cifrado e CSV), gerador de senha/passphrase + medidor de
//! força, anexos e troca de senha mestra. (O auto-type da onda 6 foi REMOVIDO
//! na onda 21 — frágil e substituído pela cópia com exclusão de histórico; a
//! camada de navegador é a direção futura, como a ponte do oficial.)
//!
//! Onda 17: **nada de operação pesada na thread da UI**. Argon2id (destrancar/
//! criar/trocar senha), PBKDF2 600k (import cifrado do Bitwarden), o keepass do
//! `.kdbx`, `fs::read` de arquivo em rede/OneDrive e os anexos rodam numa thread
//! de job (`jobs.rs`) — mesmo desenho do lab-image/lab-player: o resultado volta
//! por canal `mpsc`, a UI adota no `update` e a thread a acorda com
//! `ctx.request_repaint()` (o `egui::Context` clonado vai junto do spawn).
//!
//! Onda 23: paridade de FEATURES que faltava — URL/notas no form, export em
//! claro (`export.rs`, porte do `export.ts`), favorito ★ (com favoritos
//! primeiro na lista), lixeira (restaurar/excluir de vez) e reveal 👁 — mais a
//! repassada visual: diálogos como `egui::Modal` de verdade, campos altos,
//! botão primário em pill, ações por item em ghost button, toast no rodapé e o
//! progresso do job como card no mesmo lugar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bwdecrypt;
mod clipboard;
mod crypto;
mod export;
mod generator;
mod import;
mod jobs;
mod kdbx;
mod totp;
mod vault;

#[cfg(windows)]
mod winctl;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use eframe::egui;
use jobs::{AttachmentRead, Created, Job, JobError, JobResult, Renewed, Unlocked};
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use zeroize::Zeroizing;

#[cfg(windows)]
use tray_icon::menu::{Menu, MenuItem};
#[cfg(windows)]
use tray_icon::{TrayIcon, TrayIconBuilder};

const APP_ID: &str = "lab-keys";
const KEYRING_SERVICE: &str = "LabKeys";

/// Comando vindo da bandeja (thread própria → handlers do tray-icon).
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayCmd {
    ShowHide,
    Quit,
}

fn main() -> eframe::Result<()> {
    // ── single-instance (só Windows — é lá que o X esconde pra bandeja) ──
    // Mesmo desenho do lab-clip (onda 9): lock de ARQUIVO (`File::try_lock` —
    // o kernel libera sozinho em crash) + flag "mostra-te" consumida pela
    // thread da bandeja. Sem isso, abrir pelo ícone de novo cria uma segunda
    // instância (e segunda bandeja) em vez de acordar a que já existe.
    #[cfg(windows)]
    {
        let dir = config::config_dir(APP_ID);
        let _ = std::fs::create_dir_all(&dir);
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join("instance.lock"));
        let segunda = match &lock {
            Ok(f) => f.try_lock().is_err(),
            // Sem conseguir abrir o lock: segue SEM guarda — melhor rodar
            // sem single-instance do que não rodar.
            Err(_) => false,
        };
        if segunda {
            let _ = std::fs::write(dir.join("show.flag"), b"1");
            return Ok(()); // a bandeja do primeiro atende a flag e mostra
        }
        if let Ok(f) = lock {
            std::mem::forget(f); // lock vive até o processo morrer
        }
    }

    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Keys")
            .with_inner_size([480.0, 660.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(KeysApp::new(cfg)))
        }),
    )
}

/// Diálogo nativo só no Windows (rfd win32, sem gtk). No Linux o caminho é
/// digitado — bundlar gtk3 no AppImage engordaria dezenas de MB à toa.
#[cfg(windows)]
fn pick_open() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Cofre LocalKeys (.tkeys)", &["tkeys"])
        .pick_file()
        .map(|p| p.display().to_string())
}

#[cfg(windows)]
fn pick_save() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Cofre LocalKeys (.tkeys)", &["tkeys"])
        .set_file_name("novo-cofre.tkeys")
        .save_file()
        .map(|p| p.display().to_string())
}

/// Formato escolhido na janela de importação (a senha só é pedida nos dois
/// primeiros; CSV é texto claro).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Kdbx,
    Bitwarden,
    Csv,
}

#[cfg(windows)]
fn pick_import_file(kind: ImportKind) -> Option<String> {
    let dialog = rfd::FileDialog::new();
    let dialog = match kind {
        ImportKind::Kdbx => dialog.add_filter("KeePass (.kdbx)", &["kdbx"]),
        ImportKind::Bitwarden => dialog.add_filter("Bitwarden (.json)", &["json"]),
        ImportKind::Csv => dialog.add_filter("CSV (.csv)", &["csv"]),
    };
    dialog.pick_file().map(|p| p.display().to_string())
}

/// Windows: diálogo nativo pra escolher o anexo de origem.
#[cfg(windows)]
fn pick_open_attachment() -> Option<String> {
    rfd::FileDialog::new()
        .pick_file()
        .map(|p| p.display().to_string())
}

/// Windows: diálogo nativo pra salvar o anexo com o nome sugerido.
#[cfg(windows)]
fn pick_save_attachment(name: &str) -> Option<String> {
    rfd::FileDialog::new()
        .set_file_name(name)
        .save_file()
        .map(|p| p.display().to_string())
}

/// Formato do export em claro — o oficial só tem estes dois (`export.ts`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportKind {
    Json,
    Csv,
}

impl ExportKind {
    fn ext(self) -> &'static str {
        match self {
            ExportKind::Json => "json",
            ExportKind::Csv => "csv",
        }
    }
}

/// Windows: diálogo nativo do destino do export (sugere `cofre.json/csv`).
#[cfg(windows)]
fn pick_save_export(kind: ExportKind) -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Cofre em claro", &[kind.ext()])
        .set_file_name(format!("cofre.{}", kind.ext()))
        .save_file()
        .map(|p| p.display().to_string())
}

/// MIME mínimo por extensão (o anexo vai pro JSON do vault; sem adivinhar
/// demais, o resto cai em `application/octet-stream`).
fn mime_by_ext(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "zip" => "application/zip",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// Tamanho legível pra lista de anexos (o JSON guarda bytes crus).
fn human_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}

/// Nome do item pelo id — só pro feedback ("fulano — restaurado"). Vazio se
/// o id não existir mais.
fn item_name(raw: &serde_json::Value, id: &str) -> String {
    raw.get("items")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|it| it.get("id").and_then(serde_json::Value::as_str) == Some(id))
        })
        .and_then(|it| it.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Rótulo da escala zxcvbn (0–4) → chave i18n.
fn strength_key(score: u8) -> Key {
    match score {
        0 => Key::Strength0,
        1 => Key::Strength1,
        2 => Key::Strength2,
        3 => Key::Strength3,
        _ => Key::Strength4,
    }
}

/// Vermelho → verde, um degrau por score.
fn strength_color(score: u8) -> egui::Color32 {
    match score {
        0 => egui::Color32::from_rgb(220, 60, 60),
        1 => egui::Color32::from_rgb(230, 130, 40),
        2 => egui::Color32::from_rgb(215, 190, 60),
        3 => egui::Color32::from_rgb(120, 190, 80),
        _ => egui::Color32::from_rgb(60, 170, 90),
    }
}

// ── kit de UI (onda 23) ────────────────────────────────────────────────────
// Versão SIMPLIFICADA do `widgets.rs` do local-pdf: mesmo desenho (modal com
// cartão, campo alto, pill, ghost, toast no rodapé), sem o sistema de tokens/
// motion/bidi de lá. As cores saem do tema aplicado (`theme::apply`), então o
// mesmo desenho funciona nas 5 paletas do lab.

/// Recuo interno de um campo — molde do `FIELD_MARGIN` do local-pdf.
const FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);
/// Altura externa mínima de um campo de uma linha.
const FIELD_MIN_HEIGHT: f32 = 30.0;
/// Quanto tempo o toast fica na tela (o mesmo do local-pdf).
const TOAST_LIFETIME: std::time::Duration = std::time::Duration::from_millis(3500);

/// Cores derivadas do tema vigente (nada fixo: o lab tem 5 paletas).
#[derive(Clone, Copy)]
struct Skin {
    /// Fundo do cartão do diálogo (o "sunken" da paleta).
    card: egui::Color32,
    /// Fundo dos campos DENTRO do diálogo (o "base", o inverso do cartão).
    field: egui::Color32,
    accent: egui::Color32,
    /// Texto sobre `accent` (pill primário).
    on_accent: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    border: egui::Color32,
    /// Lavagem de hover (ghost buttons).
    hover: egui::Color32,
    /// Cartão invertido do toast/progresso (claro no tema escuro e vice-versa).
    inverted_fill: egui::Color32,
    inverted_text: egui::Color32,
}

impl Skin {
    fn of(ui: &egui::Ui) -> Self {
        Self::from_visuals(ui.visuals())
    }

    fn from_visuals(v: &egui::Visuals) -> Self {
        let text = v
            .override_text_color
            .unwrap_or(v.widgets.noninteractive.fg_stroke.color);
        let accent = v.selection.bg_fill;
        Self {
            card: v.extreme_bg_color,
            field: v.panel_fill,
            accent,
            on_accent: if luminance(accent) > 150.0 {
                egui::Color32::BLACK
            } else {
                egui::Color32::WHITE
            },
            text,
            muted: text.gamma_multiply(0.6),
            border: if v.dark_mode {
                egui::Color32::from_white_alpha(30)
            } else {
                egui::Color32::from_black_alpha(30)
            },
            hover: if v.dark_mode {
                egui::Color32::from_white_alpha(18)
            } else {
                egui::Color32::from_black_alpha(14)
            },
            inverted_fill: if v.dark_mode {
                egui::Color32::from_rgb(0xEC, 0xEC, 0xEF)
            } else {
                egui::Color32::from_rgb(0x2A, 0x2A, 0x2F)
            },
            inverted_text: if v.dark_mode {
                egui::Color32::from_rgb(0x22, 0x22, 0x26)
            } else {
                egui::Color32::WHITE
            },
        }
    }
}

/// Luminância perceptual (0–255) — decide texto claro ou escuro sobre o accent.
fn luminance(c: egui::Color32) -> f32 {
    0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
}

/// Campo de uma linha com altura de clique decente.
fn line(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .margin(FIELD_MARGIN)
        .min_size(egui::vec2(0.0, FIELD_MIN_HEIGHT))
}

/// Campo multilinha (notas) com o mesmo recuo horizontal.
fn block(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text).margin(FIELD_MARGIN)
}

/// Moldura do diálogo: cartão com canto de 10, borda sutil e a sombra do tema.
fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    let style = ctx.style();
    let skin = Skin::from_visuals(&style.visuals);
    egui::Frame::NONE
        .fill(skin.card)
        .stroke(egui::Stroke::new(1.0_f32, skin.border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(20, 16))
        .shadow(style.visuals.window_shadow)
}

/// Visuals de dentro do diálogo: campos com contorno e no tom "base" (o cartão
/// é o "sunken" — sem isso o campo some no fundo do modal).
fn begin_dialog(ui: &mut egui::Ui) {
    let skin = Skin::of(ui);
    let v = ui.visuals_mut();
    v.extreme_bg_color = skin.field;
    v.selection.stroke = egui::Stroke::new(1.5_f32, skin.accent);
    let w = &mut v.widgets;
    w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, skin.border);
    w.inactive.weak_bg_fill = skin.field;
    w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, skin.muted);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = egui::CornerRadius::same(4);
    }
    ui.spacing_mut().interact_size.y = FIELD_MIN_HEIGHT;
}

/// Botão pill: `primary` preenche com o accent, o secundário fica no contorno.
/// Desenhado à mão porque o `egui::Button` não tem raio/altura própria.
fn pill(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let skin = Skin::of(ui);
    if primary {
        pill_colored(ui, label, skin.accent, skin.on_accent, false)
    } else {
        pill_colored(ui, label, egui::Color32::TRANSPARENT, skin.text, true)
    }
}

/// Pill de ação destrutiva (vermelho fixo: legível nos dois temas).
fn pill_danger(ui: &mut egui::Ui, label: &str) -> egui::Response {
    pill_colored(
        ui,
        label,
        egui::Color32::from_rgb(0xC0, 0x3A, 0x3A),
        egui::Color32::WHITE,
        false,
    )
}

/// Pill com cores explícitas; `outline` desenha só a borda.
fn pill_colored(
    ui: &mut egui::Ui,
    label: &str,
    fill: egui::Color32,
    ink: egui::Color32,
    outline: bool,
) -> egui::Response {
    let skin = Skin::of(ui);
    let enabled = ui.is_enabled();
    let ink = if enabled { ink } else { skin.muted };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 28.0, 28.0),
        egui::Sense::click(),
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let hovered = resp.hovered() && enabled;
    let fill = if !enabled {
        fill.gamma_multiply(0.45)
    } else if hovered {
        fill.gamma_multiply(1.12)
    } else {
        fill.gamma_multiply(0.9)
    };
    let fill = if outline {
        if hovered {
            skin.hover
        } else {
            egui::Color32::TRANSPARENT
        }
    } else {
        fill
    };
    let stroke = if outline {
        egui::Stroke::new(1.0_f32, skin.border)
    } else {
        egui::Stroke::NONE
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(14),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

/// Ação de linha: transparente até o hover (o "ghost button" do local-pdf).
fn ghost(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ghost_ink(ui, label, Skin::of(ui).muted)
}

/// [`ghost`] com tinta própria (o ★ favorito usa o accent quando ativo).
fn ghost_ink(ui: &mut egui::Ui, label: &str, ink: egui::Color32) -> egui::Response {
    let skin = Skin::of(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 14.0, 24.0),
        egui::Sense::click(),
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), skin.hover);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, ink);
    resp
}

/// `dd/mm/aaaa` a partir de epoch-ms — o lab não tem crate de data, e o único
/// uso é mostrar quando o item foi pra lixeira.
fn date_from_ms(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Algoritmo civil-from-days (Hinnant), o mesmo que o chrono usa.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}/{m:02}/{y:04}")
}

/// Export cifrado do Bitwarden traz `passwordProtected` (ou `encrypted`) true.
/// Checamos ANTES de decifrar pra não chamar o decrypt num JSON claro.
fn bitwarden_encrypted(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|v| {
            v.get("passwordProtected")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                || v.get("encrypted").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .unwrap_or(false)
}

/// Último cofre aberto (pra pré-preencher o caminho e alimentar o quick mode).
fn state_path() -> std::path::PathBuf {
    config::config_dir(APP_ID).join("state.json")
}

fn remember_last_path(path: &str) {
    let p = state_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, serde_json::json!({ "lastPath": path }).to_string());
}

fn last_path() -> Option<String> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(state_path()).ok()?).ok()?;
    v.get("lastPath")?.as_str().map(str::to_string)
}

/// Invalida a entrada do desbloqueio rápido no keyring: a chave guardada foi
/// derivada do salt ANTIGO (a troca de senha recifra com salt novo), então ela
/// não abre mais o cofre.
#[cfg(windows)]
fn invalidate_quick_key(path: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, path) {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(windows))]
fn invalidate_quick_key(_path: &str) {}

// ── bandeja (Windows) ──────────────────────────────────────────────────────

/// Sobe a bandeja + a thread que atende os comandos dela. Devolve o `TrayIcon`
/// (que o app segura vivo) e a flag de quit lida no `update`.
///
/// A thread é obrigatória: com a janela oculta o eframe congela (sem WM_PAINT
/// não há frame), então os handlers do tray-icon não podem depender do
/// `update` — mostram/escondem via `winctl` (SO direto). Encerrar é sempre
/// "mostrar + flag de quit": o update, com a janela visível, faz o
/// `ViewportCommand::Close` — caminho limpo do winit 0.30.
#[cfg(windows)]
fn spawn_tray() -> (Option<TrayIcon>, Arc<AtomicBool>) {
    let (tx, rx) = std::sync::mpsc::channel::<TrayCmd>();
    let quit = Arc::new(AtomicBool::new(false));

    let show = MenuItem::with_id("show", "Mostrar/Ocultar", true, None);
    let quit_item = MenuItem::with_id("quit", "Sair", true, None);
    let menu = Menu::new();
    let _ = menu.append(&show);
    let _ = menu.append(&quit_item);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Lab Keys")
        .with_icon(tray_icon().expect("ícone rgba"))
        .build();

    let tray = match tray {
        Ok(t) => {
            {
                use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
                let tx = tx.clone();
                tray_icon::TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                    {
                        let _ = tx.send(TrayCmd::ShowHide);
                    }
                }));
            }
            {
                use tray_icon::menu::MenuEvent;
                let tx = tx.clone();
                MenuEvent::set_event_handler(Some(move |ev: MenuEvent| match ev.id.0.as_str() {
                    "show" => {
                        let _ = tx.send(TrayCmd::ShowHide);
                    }
                    "quit" => {
                        let _ = tx.send(TrayCmd::Quit);
                    }
                    _ => {}
                }));
            }
            Some(t)
        }
        Err(e) => {
            // Sem bandeja o app segue abrindo normal (o X fecha de verdade).
            eprintln!("bandeja: não deu para criar: {e}");
            None
        }
    };

    let quit_flag = quit.clone();
    std::thread::Builder::new()
        .name("keys-tray".into())
        .spawn(move || {
            // Flag "mostra-te" da segunda instância (single-instance): a
            // thread da bandeja é quem a consome — o update não roda com a
            // janela oculta, então não pode depender dele.
            let show_flag = config::config_dir(APP_ID).join("show.flag");
            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(cmd) => match cmd {
                        TrayCmd::ShowHide => {
                            if winctl::is_visible() {
                                winctl::hide();
                            } else {
                                winctl::show();
                            }
                        }
                        TrayCmd::Quit => {
                            quit_flag.store(true, Ordering::Relaxed);
                            // Janela VISÍVEL primeiro — o update precisa rodar pra
                            // encerrar pelo caminho limpo (Close).
                            winctl::show();
                        }
                    },
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                if show_flag.exists() {
                    let _ = std::fs::remove_file(&show_flag);
                    winctl::show();
                }
            }
        })
        .expect("spawn keys-tray");

    (tray, quit)
}

/// Ícone 32×32 gerado em código (lab não carrega assets): chave — cabeça
/// redonda + haste com dente, nas cores da suíte.
#[cfg(windows)]
fn tray_icon() -> Option<tray_icon::Icon> {
    let (w, h) = (32u32, 32u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            // círculo da cabeça (centro 11,13, raio 7) — preenchido com furo (raio 3)
            let dx = x as i32 - 11;
            let dy = y as i32 - 13;
            let dist2 = dx * dx + dy * dy;
            let head = dist2 <= 49 && dist2 >= 9;
            // haste horizontal saindo da cabeça, com dente pra baixo
            let shaft = y >= 12 && y <= 14 && x >= 17 && x < w - 3;
            let tooth = x >= 22 && x <= 24 && y >= 12 && y <= 19;
            let (r, g, b, a) = if head || shaft || tooth {
                (90, 50, 70, 255)
            } else {
                (0, 0, 0, 0)
            };
            let i = ((y * w + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    tray_icon::Icon::from_rgba(rgba, w, h).ok()
}

struct KeysApp {
    cfg: Config,
    // trancado
    path: String,
    master: String,
    confirm: String,
    err: Option<String>,
    quick_unlock: bool,
    quick_ativa: bool, // há chave guardada no cofre do SO pra este path
    // destrancado
    session: Option<crypto::SessionKey>,
    raw: Option<serde_json::Value>,
    items: Vec<vault::ItemView>,
    search: String,
    /// Filtro "só códigos que mudam" (TOTP/oauth otp): mostra só itens com
    /// segredo TOTP. Estado de sessão — não persiste, como a busca.
    filter_totp: bool,
    // formulário de adição/edição
    form_open: bool,
    form_edit_id: Option<String>,
    add_name: String,
    add_user: String,
    add_pass: String,
    add_totp: String,
    /// Primeira URI do item (o form edita só ela, como o oficial).
    add_url: String,
    add_notes: String,
    /// 👁 do campo de senha do form (volta a ocultar ao fechar).
    reveal_form: bool,
    /// 👁 da senha do arquivo de import.
    reveal_import: bool,
    /// Exclusão esperando confirmação (um clique sem querer não apaga).
    delete_ask: Option<(String, String)>,
    // onda 23 — lixeira
    trash_open: bool,
    /// "Excluir de vez" esperando o segundo clique (o `confirm()` do oficial).
    purge_ask: Option<String>,
    // onda 23 — export em claro
    export_open: bool,
    export_kind: ExportKind,
    export_path: String,
    // onda 6 — importação
    import_open: bool,
    import_kind: ImportKind,
    import_path: String,
    import_password: String,
    /// `(importados, pulados card/identity)` — Some = janela de relatório.
    import_report: Option<(usize, usize)>,
    // onda 6 — gerador (defaults do oficial)
    gen_open: bool,
    gen_passphrase: bool,
    gen_pw: generator::PasswordOptions,
    gen_pp: generator::PassphraseOptions,
    gen_result: String,
    // onda 6 — troca de senha mestra
    change_open: bool,
    chg_old: String,
    chg_new: String,
    chg_confirm: String,
    // onda 6 — anexos (id do item com a janela aberta + caminho digitável)
    attach_item: Option<String>,
    attach_path: String,
    /// Toast do rodapé: `(mensagem, quando apareceu)`. Um só canal de
    /// feedback — "copiado", "senha trocada", "exportado" caem todos aqui.
    toast: Option<(String, std::time::Instant)>,
    /// Canal do job pesado em andamento (`jobs.rs`). Um por vez: enquanto
    /// `job_label` é Some, os gatilhos de job ficam desabilitados.
    job_rx: Option<Receiver<JobResult>>,
    /// O que está rodando (chave i18n do spinner).
    job_label: Option<Key>,
    /// Viva enquanto o app viver: derruba a bandeja no drop.
    #[cfg(windows)]
    _tray: Option<TrayIcon>,
    /// "Sair" da bandeja: o update responde com `ViewportCommand::Close`
    /// (o único caminho que encerra de verdade).
    quit: Arc<AtomicBool>,
}

impl KeysApp {
    fn new(cfg: Config) -> Self {
        let mut app = Self {
            cfg,
            path: String::new(),
            master: String::new(),
            confirm: String::new(),
            err: None,
            quick_unlock: false,
            quick_ativa: false,
            session: None,
            raw: None,
            items: Vec::new(),
            search: String::new(),
            filter_totp: false,
            form_open: false,
            form_edit_id: None,
            add_name: String::new(),
            add_user: String::new(),
            add_pass: String::new(),
            add_totp: String::new(),
            add_url: String::new(),
            add_notes: String::new(),
            reveal_form: false,
            reveal_import: false,
            delete_ask: None,
            trash_open: false,
            purge_ask: None,
            export_open: false,
            export_kind: ExportKind::Json,
            export_path: String::new(),
            import_open: false,
            import_kind: ImportKind::Kdbx,
            import_path: String::new(),
            import_password: String::new(),
            import_report: None,
            gen_open: false,
            gen_passphrase: false,
            gen_pw: generator::PasswordOptions::default(),
            gen_pp: generator::PassphraseOptions::default(),
            gen_result: String::new(),
            change_open: false,
            chg_old: String::new(),
            chg_new: String::new(),
            chg_confirm: String::new(),
            attach_item: None,
            attach_path: String::new(),
            toast: None,
            job_rx: None,
            job_label: None,
            #[cfg(windows)]
            _tray: None,
            quit: Arc::new(AtomicBool::new(false)),
        };
        #[cfg(windows)]
        {
            let (tray, quit) = spawn_tray();
            app._tray = tray;
            app.quit = quit;
        }
        app.path = last_path().unwrap_or_default();
        app.quick_ativa = app.path_has_stored_key();
        app.try_quick_unlock();
        app
    }

    fn locked(&self) -> bool {
        self.session.is_none()
    }

    /// Tranca: apaga a sessão (Drop → Zeroizing limpa a chave) e o vault em
    /// claro da memória. Ordem session → raw, como no oficial.
    fn lock(&mut self) {
        self.session = None;
        self.raw = None;
        self.items.clear();
        self.master.clear();
        self.confirm.clear();
        self.add_pass.clear();
        self.form_open = false;
        self.form_edit_id = None;
        self.add_url.clear();
        self.add_notes.clear();
        self.reveal_form = false;
        self.delete_ask = None;
        self.trash_open = false;
        self.purge_ask = None;
        self.export_open = false;
        self.export_path.clear();
        // Onda 6: nenhum segredo de import/gerador/senha mestra sobrevive ao
        // trancar, e as janelas fecham junto.
        self.import_open = false;
        self.import_path.clear();
        self.import_password.clear();
        self.reveal_import = false;
        self.import_report = None;
        self.gen_open = false;
        self.gen_result.clear();
        self.change_open = false;
        self.chg_old.clear();
        self.chg_new.clear();
        self.chg_confirm.clear();
        self.attach_item = None;
        self.attach_path.clear();
        self.toast = None;
    }

    fn adopt(&mut self, session: crypto::SessionKey, raw: serde_json::Value) {
        self.items = vault::items_view(&raw);
        self.raw = Some(raw);
        self.session = Some(session);
        self.err = None;
        self.master.clear();
        self.confirm.clear();
    }

    // ── desbloqueio rápido (keyring do SO — desenho do oficial) ────────
    // Só no Windows: secret-service no Linux puxaria libdbus pro build
    // (runner/AppImage sem dbus-1). Fora do Windows os stubs desligam tudo.

    #[cfg(windows)]
    fn path_has_stored_key(&self) -> bool {
        let path = self.path.trim();
        !path.is_empty()
            && keyring::Entry::new(KEYRING_SERVICE, path)
                .and_then(|e| e.get_password())
                .is_ok()
    }

    #[cfg(not(windows))]
    fn path_has_stored_key(&self) -> bool {
        false
    }

    /// Tenta abrir o último cofre com a chave guardada no cofre do SO, sem
    /// pedir a master. Falha silenciosa (chave inválida → apaga a entrada:
    /// senha mudou em outro lugar, a chave velha não serve mais).
    ///
    /// Fica SÍNCRONO de propósito: a chave já está derivada, então
    /// `open_with_key` é só o AEAD de poucos KB (sem Argon2) — não há KDF pra
    /// tirar da thread da UI.
    #[cfg(windows)]
    fn try_quick_unlock(&mut self) {
        use base64::Engine as _;

        let Some(path) = last_path() else {
            return;
        };
        let Ok(b64) =
            (|| keyring::Entry::new(KEYRING_SERVICE, &path).and_then(|e| e.get_password()))()
        else {
            return;
        };
        let Ok(key) = base64::engine::general_purpose::STANDARD.decode(b64) else {
            return;
        };
        let Ok(mut key) = <[u8; 32]>::try_from(key) else {
            return;
        };
        let Ok(file) = std::fs::read(&path) else {
            return;
        };
        match crypto::open_with_key(&key, &file) {
            Ok((plaintext, session)) => {
                match serde_json::from_slice::<serde_json::Value>(&plaintext) {
                    Ok(raw) => {
                        self.path = path.clone();
                        self.adopt(session, raw);
                        remember_last_path(&path);
                    }
                    Err(_) => self.err = Some("vault não é JSON válido".into()),
                }
            }
            Err(_) => {
                // chave guardada não serve mais (senha trocada / arquivo outro)
                if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &path) {
                    let _ = entry.delete_credential();
                }
            }
        }
        key.fill(0);
        self.quick_ativa = self.path_has_stored_key();
    }

    #[cfg(not(windows))]
    fn try_quick_unlock(&mut self) {}

    #[cfg(windows)]
    fn store_quick_key(&self, session: &crypto::SessionKey) {
        use base64::Engine as _;

        let path = self.path.trim();
        if path.is_empty() {
            return;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(session.key_bytes());
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, path) {
            let _ = entry.set_password(&b64);
        }
    }

    #[cfg(not(windows))]
    fn store_quick_key(&self, _session: &crypto::SessionKey) {}

    #[cfg(windows)]
    fn forget_quick_key(&mut self) {
        let path = self.path.trim();
        if !path.is_empty() {
            if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, path) {
                let _ = entry.delete_credential();
            }
        }
        self.quick_unlock = false;
        self.quick_ativa = false;
    }

    #[cfg(not(windows))]
    fn forget_quick_key(&mut self) {
        self.quick_unlock = false;
        self.quick_ativa = false;
    }

    // ── abrir / criar / salvar ──────────────────────────────────────────

    /// Destranca: a validação é barata (fica aqui); ler o arquivo e rodar o
    /// Argon2id vão pra thread. A master sai do campo como `Zeroizing` inteiro
    /// (a thread é dona dela e a apaga ao terminar).
    fn unlock(&mut self, ctx: &egui::Context) {
        let path = self.path.trim().to_string();
        if path.is_empty() || self.master.is_empty() {
            return;
        }
        let password = Zeroizing::new(std::mem::take(&mut self.master));
        let quick = self.quick_unlock;
        self.start_job(
            ctx,
            Key::Unlocking,
            Job::Unlock {
                path,
                password,
                quick,
            },
        );
    }

    fn create(&mut self, ctx: &egui::Context) {
        if self.path.trim().is_empty() {
            return;
        }
        if self.master.is_empty() {
            self.err = Some("a senha mestra não pode ser vazia".into());
            return;
        }
        if self.master != self.confirm {
            self.err = Some("as senhas não conferem".into());
            return;
        }
        let password = Zeroizing::new(std::mem::take(&mut self.master));
        self.confirm.clear();
        let quick = self.quick_unlock;
        let path = self.path.trim().to_string();
        self.start_job(
            ctx,
            Key::Working,
            Job::Create {
                path,
                password,
                quick,
            },
        );
    }

    /// Cifra + grava um novo estado do vault. Se o salvamento falhar, o raw em
    /// memória NÃO muda — memória e disco nunca divergem. Devolve ok/falha.
    fn persist(&mut self, next: serde_json::Value) -> bool {
        let bytes = match serde_json::to_vec(&next) {
            Ok(b) => b,
            Err(e) => {
                self.err = Some(e.to_string());
                return false;
            }
        };
        let sealed = match self.session.as_ref().map(|s| s.seal(&bytes)) {
            Some(Ok(file)) => file,
            Some(Err(e)) => {
                self.err = Some(e.to_string());
                return false;
            }
            None => return false,
        };
        match vault::atomic_write(std::path::Path::new(self.path.trim()), &sealed) {
            Ok(()) => {
                self.err = None;
                self.items = vault::items_view(&next);
                self.raw = Some(next);
                true
            }
            Err(e) => {
                self.err = Some(e);
                false
            }
        }
    }

    /// Salva o item do formulário (novo ou edição) e persiste.
    fn save_form(&mut self) {
        let name = self.add_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        // Owned antes: o `LoginForm` empresta estes buffers e o `persist`
        // precisa de `&mut self`.
        let username = self.add_user.clone();
        let password = self.add_pass.clone();
        let totp = self.add_totp.trim().to_string();
        let url = self.add_url.trim().to_string();
        let notes = self.add_notes.clone();
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        let form = vault::LoginForm {
            name: &name,
            username: &username,
            password: &password,
            totp: &totp,
            url: &url,
            notes: &notes,
        };
        let ok = match self.form_edit_id.clone() {
            Some(id) => vault::edit_login(&mut next, &id, form),
            None => {
                vault::add_login(&mut next, form);
                true
            }
        };
        if !ok {
            self.err = Some("item não encontrado".into());
            return;
        }
        if self.persist(next) {
            self.add_name.clear();
            self.add_user.clear();
            self.add_pass.clear();
            self.add_totp.clear();
            self.add_url.clear();
            self.add_notes.clear();
            self.form_open = false;
            self.form_edit_id = None;
        }
    }

    /// Abre o form em modo "novo item" (o "+" do cabeçalho).
    fn open_add_form(&mut self) {
        self.form_edit_id = None;
        self.add_name.clear();
        self.add_user.clear();
        self.add_pass.clear();
        self.add_totp.clear();
        self.add_url.clear();
        self.add_notes.clear();
        self.reveal_form = false;
        self.form_open = true;
    }

    /// Abre o form carregando o item (inclui `uris[0]` e as notas).
    fn open_edit_form(&mut self, id: &str) {
        let Some(full) = self.raw.as_ref().and_then(|r| vault::login_full(r, id)) else {
            return;
        };
        self.form_edit_id = Some(id.to_string());
        self.add_name = self
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        self.add_user = full.username;
        self.add_pass = full.password;
        self.add_totp = full.totp;
        self.add_url = full.url;
        self.add_notes = full.notes;
        self.reveal_form = false;
        self.form_open = true;
    }

    fn close_form(&mut self) {
        self.form_open = false;
        self.form_edit_id = None;
        self.add_name.clear();
        self.add_user.clear();
        self.add_pass.clear();
        self.add_totp.clear();
        self.add_url.clear();
        self.add_notes.clear();
        self.reveal_form = false;
    }

    /// Exclusão lógica (lixeira do oficial).
    fn delete_item(&mut self, id: &str) {
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        if vault::delete_login(&mut next, id) {
            self.persist(next);
        }
    }

    // ── onda 23: favorito, lixeira, export ──────────────────────────────

    /// Liga/desliga o ★ do item. O oficial não tem `toggleFavorite`: o ★ do
    /// editor passa por `updateItem`, que grava `updatedAt` — `set_favorite`
    /// faz o mesmo (ver `vault.rs`).
    fn toggle_favorite(&mut self, id: &str) {
        let Some(current) = self.items.iter().find(|i| i.id == id).map(|i| i.favorite) else {
            return;
        };
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        if vault::set_favorite(&mut next, id, !current) {
            self.persist(next);
        }
    }

    /// Restaura um item da lixeira (o `restoreItem` do oficial).
    fn restore_item(&mut self, id: &str) {
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        let name = item_name(&next, id);
        if vault::restore_item(&mut next, id) && self.persist(next) {
            let msg = format!("{name} — {}", i18n::t(self.cfg.lang, Key::Restored));
            self.notice(msg);
        }
    }

    /// Exclui de vez (o `deleteForever` do oficial, que pede confirmação).
    fn purge_item(&mut self, id: &str) {
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        if vault::delete_forever(&mut next, id) && self.persist(next) {
            self.purge_ask = None;
            let msg = i18n::t(self.cfg.lang, Key::Deleted).to_string();
            self.notice(msg);
        }
    }

    /// Export em CLARO: monta o texto aqui (barato) e manda a GRAVAÇÃO pra
    /// thread — destino em rede/OneDrive trava como qualquer escrita.
    fn run_export(&mut self, ctx: &egui::Context) {
        let target = self.export_path.trim().to_string();
        if target.is_empty() {
            self.err = Some("informe o destino do export".into());
            return;
        }
        let Some(raw) = self.raw.as_ref() else {
            return;
        };
        let text = match self.export_kind {
            ExportKind::Json => export::export_json(raw),
            ExportKind::Csv => export::export_csv(raw),
        };
        self.export_open = false;
        self.start_job(ctx, Key::Working, Job::WriteExport { target, text });
    }

    // ── onda 6: importação ──────────────────────────────────────────────

    /// Manda ler/decifrar/parsear o arquivo escolhido pra thread (PBKDF2 600k e
    /// o keepass levam SEGUNDOS). O `push_items` + `persist` ficam no retorno —
    /// o relatório (importados/pulados) abre numa janela própria.
    fn run_import(&mut self, ctx: &egui::Context) {
        let path = self.import_path.trim().to_string();
        if path.is_empty() {
            self.err = Some("informe o arquivo a importar".into());
            return;
        }
        // A senha do arquivo FICA no campo (se o import falhar por caminho
        // errado, o usuário não redigita): vai pra thread uma cópia que se
        // apaga sozinha.
        let password = Zeroizing::new(self.import_password.clone());
        self.start_job(
            ctx,
            Key::Importing,
            Job::Import {
                kind: self.import_kind,
                path,
                password,
            },
        );
    }

    // ── onda 6: troca de senha mestra ───────────────────────────────────

    fn change_master(&mut self, ctx: &egui::Context) {
        if self.chg_new.is_empty() {
            self.err = Some("a nova senha mestra não pode ser vazia".into());
            return;
        }
        if self.chg_new != self.chg_confirm {
            self.err = Some("as senhas não conferem".into());
            return;
        }
        // Os campos ficam preenchidos (erro de digitação na senha atual é
        // comum); as cópias vão pra thread sob `Zeroizing`.
        self.start_job(
            ctx,
            Key::ChangingMaster,
            Job::ChangeMaster {
                path: self.path.trim().to_string(),
                old: Zeroizing::new(self.chg_old.clone()),
                new: Zeroizing::new(self.chg_new.clone()),
            },
        );
    }

    // ── onda 6: anexos ──────────────────────────────────────────────────

    /// Lê o arquivo de origem (picker no Windows; caminho digitado no Linux) e
    /// codifica em base64 — NA THREAD: `fs::read` de arquivo grande/rede trava.
    /// O cap de `MAX_ATTACHMENT_BYTES` é checado lá antes de ler. O blob do
    /// cofre já cifra tudo.
    fn add_attachment_from(&mut self, item_id: &str, ctx: &egui::Context) {
        #[cfg(windows)]
        let source = pick_open_attachment();
        #[cfg(not(windows))]
        let source = {
            let p = self.attach_path.trim();
            (!p.is_empty()).then(|| p.to_string())
        };
        let Some(source) = source else {
            self.err = Some("informe o arquivo do anexo".into());
            return;
        };
        self.start_job(
            ctx,
            Key::ReadingFile,
            Job::ReadAttachment {
                item_id: item_id.to_string(),
                source,
            },
        );
    }

    /// Decodifica o base64 do anexo e grava no destino escolhido — na thread
    /// (`fs::write` em pasta de rede tem a mesma lentidão da leitura).
    fn save_attachment(&mut self, item_id: &str, attachment_id: &str, ctx: &egui::Context) {
        let Some((name, b64)) = self
            .raw
            .as_ref()
            .and_then(|r| vault::attachment_data(r, item_id, attachment_id))
        else {
            self.err = Some("anexo não encontrado".into());
            return;
        };
        #[cfg(windows)]
        let target = pick_save_attachment(&name);
        #[cfg(not(windows))]
        let target = {
            let p = self.attach_path.trim();
            (!p.is_empty()).then(|| p.to_string())
        };
        let Some(target) = target else {
            self.err = Some("informe o destino do anexo".into());
            return;
        };
        self.start_job(
            ctx,
            Key::Working,
            Job::SaveAttachment {
                target,
                data_b64: b64,
            },
        );
    }

    // ── feedback ────────────────────────────────────────────────────────

    /// "copiado" — o toast mostra `✓ Copiar — <o quê>`.
    fn copied(&mut self, what: &str) {
        let msg = format!("{} — {what}", i18n::t(self.cfg.lang, Key::Copy));
        self.notice(msg);
    }

    /// Toast do rodapé (3,5 s): um canal só pra todo feedback.
    fn notice(&mut self, msg: String) {
        self.toast = Some((msg, std::time::Instant::now()));
    }

    /// Rodapé: card do job em andamento (spinner + rótulo) e o toast, ambos
    /// centrados embaixo — o desenho do `toast`/`progress_notice` do local-pdf,
    /// com um cartão invertido em relação ao tema.
    fn footer(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        let skin = Skin::from_visuals(&ctx.style().visuals);
        let screen = ctx.screen_rect();
        let mut lift = 18.0;
        if let Some(label) = self.job_label {
            let area = egui::Area::new(egui::Id::new("job-notice"))
                .order(egui::Order::Tooltip)
                .pivot(egui::Align2::CENTER_BOTTOM)
                .fixed_pos(screen.center_bottom() - egui::vec2(0.0, lift))
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(skin.inverted_fill)
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(16, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().color(skin.inverted_text));
                                ui.label(egui::RichText::new(t(label)).color(skin.inverted_text));
                            });
                        });
                });
            lift += area.response.rect.height() + 8.0;
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some((msg, at)) = self.toast.clone() else {
            return;
        };
        if at.elapsed() > TOAST_LIFETIME {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Tooltip)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(screen.center_bottom() - egui::vec2(0.0, lift))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(skin.inverted_fill)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        // Um toast curto não pode impor largura ao próximo.
                        ui.set_max_width((screen.width() - 64.0).clamp(1.0, 560.0));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("✓ {msg}")).color(skin.inverted_text),
                            )
                            .wrap(),
                        );
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    // ── jobs (onda 17) ──────────────────────────────────────────────────

    /// Manda o job pra thread e fica devendo o resultado. Um job por vez: os
    /// gatilhos só chamam isto com `job_label` vazio.
    fn start_job(&mut self, ctx: &egui::Context, label: Key, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        // O `Context` é Send+Sync: a thread chama `request_repaint()` no fim e
        // o resultado aparece sem o usuário mexer no mouse.
        jobs::spawn(job, tx, ctx.clone());
        self.job_rx = Some(rx);
        self.job_label = Some(label);
        self.err = None;
        ctx.request_repaint();
    }

    /// Drena o canal no início do frame e aplica o que chegou. O canal é
    /// `take`-ado pra soltar o borrow antes de mexer no estado.
    fn poll_jobs(&mut self) {
        let Some(rx) = self.job_rx.take() else {
            return;
        };
        let (results, closed) = jobs::drain(&rx);
        for result in results {
            self.apply_job(result);
        }
        if closed {
            // Thread terminou sem resultado (spawn falhou / panic): não deixa o
            // spinner girando pra sempre.
            if self.job_label.take().is_some() {
                self.err = Some("operação interrompida sem resultado".into());
            }
        } else {
            self.job_rx = Some(rx);
        }
    }

    /// Aplica o resultado do job — aqui (e só aqui) o estado do app muda.
    fn apply_job(&mut self, result: JobResult) {
        self.job_label = None;
        match result {
            JobResult::Unlocked(Ok(unlocked)) => {
                let Unlocked {
                    path,
                    raw,
                    session,
                    quick,
                } = unlocked;
                if quick {
                    self.store_quick_key(&session);
                }
                self.path = path.clone();
                self.adopt(session, raw);
                remember_last_path(&path);
                self.quick_ativa = self.path_has_stored_key();
            }
            JobResult::Unlocked(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::Created(Ok(created)) => {
                let Created {
                    path,
                    file,
                    raw,
                    session,
                    quick,
                } = created;
                // Gravação atômica aqui: poucos KB e é ela que decide se o
                // cofre novo é adotado (disco e memória não divergem).
                match vault::atomic_write(std::path::Path::new(&path), &file) {
                    Ok(()) => {
                        if quick {
                            self.store_quick_key(&session);
                        }
                        self.path = path.clone();
                        self.adopt(session, raw);
                        remember_last_path(&path);
                    }
                    Err(e) => self.err = Some(e),
                }
            }
            JobResult::Created(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::MasterChanged(Ok(renewed)) => {
                let Renewed { path, file } = renewed;
                match vault::atomic_write(std::path::Path::new(&path), &file) {
                    Ok(()) => {
                        // A sessão em memória foi derivada do salt ANTIGO: um
                        // save reescreveria o arquivo com a senha velha.
                        // Trancar força o destrancar com a nova (o "vault
                        // fechado no disco" que o oficial assume).
                        self.lock();
                        // A chave do keyring foi derivada do salt antigo.
                        invalidate_quick_key(&path);
                        self.quick_ativa = false;
                        self.err = None;
                        let msg = i18n::t(self.cfg.lang, Key::MasterChanged).to_string();
                        self.notice(msg);
                    }
                    Err(e) => self.err = Some(e),
                }
            }
            JobResult::MasterChanged(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::Imported(Ok(imported)) => {
                let Some(mut next) = self.raw.clone() else {
                    return;
                };
                let count = imported.items.len();
                vault::push_items(&mut next, imported.items);
                if self.persist(next) {
                    self.import_report = Some((count, imported.skipped));
                    self.import_open = false;
                    self.import_path.clear();
                    self.import_password.clear();
                }
            }
            JobResult::Imported(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::AttachmentRead(Ok(read)) => self.apply_attachment(read),
            JobResult::AttachmentRead(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::AttachmentSaved(Ok(target)) => {
                self.err = None;
                let msg = format!("{}: {target}", i18n::t(self.cfg.lang, Key::SaveAttachment));
                self.notice(msg);
            }
            JobResult::AttachmentSaved(Err(e)) => self.err = Some(self.job_error(e)),
            JobResult::Exported(Ok(target)) => {
                self.err = None;
                let msg = format!("{}: {target}", i18n::t(self.cfg.lang, Key::Exported));
                self.notice(msg);
            }
            JobResult::Exported(Err(e)) => self.err = Some(self.job_error(e)),
        }
    }

    fn apply_attachment(&mut self, read: AttachmentRead) {
        let AttachmentRead {
            item_id,
            name,
            mime,
            size,
            data_b64,
        } = read;
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        if vault::add_attachment(&mut next, &item_id, &name, mime, &data_b64) {
            if self.persist(next) {
                let msg = format!("{name} — {}", human_size(size));
                self.notice(msg);
            }
        } else {
            self.err = Some("não foi possível anexar ao item".into());
        }
    }

    /// Erro de job → texto pro `self.err` (o que é traduzível volta como
    /// variante porque a thread não conhece o idioma).
    fn job_error(&self, e: JobError) -> String {
        match e {
            JobError::Msg(msg) => msg,
            JobError::WrongPassword => i18n::t(self.cfg.lang, Key::WrongPassword).to_string(),
            JobError::AttachmentTooBig { name } => {
                format!("{name}: {}", i18n::t(self.cfg.lang, Key::AttachmentTooBig))
            }
        }
    }
}

/// Linha da lista (owned antes do UI: nenhum borrow de `self.items` nas
/// closures) — com a ordenação de renderização aplicada em cima.
struct Row {
    id: String,
    name: String,
    username: Option<String>,
    totp: Option<String>,
    favorite: bool,
}

impl KeysApp {
    // ── onda 23: diálogos ───────────────────────────────────────────────

    /// Form de adição/edição: nome, usuário, senha (com 👁), URL, TOTP e notas.
    fn form_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.form_open {
            return;
        }
        let title = if self.form_edit_id.is_some() {
            t(Key::Edit)
        } else {
            t(Key::Add)
        };
        let mut cancel = false;
        let resp = egui::Modal::new(egui::Id::new("form"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(400.0);
                ui.heading(title);
                ui.add_space(10.0);
                egui::Grid::new("form-grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t(Key::Name));
                        ui.add(line(&mut self.add_name).desired_width(280.0));
                        ui.end_row();

                        ui.label(t(Key::Username));
                        ui.add(line(&mut self.add_user).desired_width(280.0));
                        ui.end_row();

                        ui.label(t(Key::Password));
                        ui.horizontal(|ui| {
                            ui.add(
                                line(&mut self.add_pass)
                                    .password(!self.reveal_form)
                                    .desired_width(252.0),
                            );
                            // 👁 reveal: o campo volta a ocultar ao fechar.
                            if ghost(ui, if self.reveal_form { "🙈" } else { "👁" })
                                .on_hover_text(t(Key::Reveal))
                                .clicked()
                            {
                                self.reveal_form = !self.reveal_form;
                            }
                        });
                        ui.end_row();

                        ui.label(t(Key::Url));
                        ui.add(
                            line(&mut self.add_url)
                                .hint_text("https://…")
                                .desired_width(280.0),
                        );
                        ui.end_row();

                        ui.label(t(Key::Totp));
                        ui.add(
                            line(&mut self.add_totp)
                                .hint_text("base32 (opcional)")
                                .desired_width(280.0),
                        );
                        ui.end_row();

                        ui.label(t(Key::Notes));
                        ui.add(
                            block(&mut self.add_notes)
                                .desired_width(280.0)
                                .desired_rows(3),
                        );
                        ui.end_row();
                    });
                // Força ao vivo (zxcvbn) — só quando há senha.
                if !self.add_pass.is_empty() {
                    let strength =
                        generator::password_strength(&self.add_pass, &[self.add_name.as_str()]);
                    ui.add_space(8.0);
                    ui.add(
                        egui::ProgressBar::new((f32::from(strength.score) + 1.0) / 5.0)
                            .desired_width(280.0)
                            .fill(strength_color(strength.score))
                            .text(t(strength_key(strength.score))),
                    )
                    .on_hover_text(format!(
                        "zxcvbn: 10^{:.1} tentativas",
                        strength.guesses_log10
                    ));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    // Atalho pro gerador (abre por cima deste modal).
                    if pill(ui, t(Key::Generate), false).clicked() {
                        self.gen_open = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_save = !self.add_name.trim().is_empty();
                        let saved = ui
                            .add_enabled_ui(can_save, |ui| pill(ui, t(Key::Save), true))
                            .inner
                            .clicked();
                        if saved {
                            self.save_form();
                        }
                        if pill(ui, t(Key::Cancel), false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if cancel || resp.should_close() {
            self.close_form();
        }
    }

    /// Confirmação da exclusão (vai pra lixeira, mas não sem querer).
    fn delete_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        let Some((id, name)) = self.delete_ask.clone() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let resp = egui::Modal::new(egui::Id::new("delete"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(320.0);
                ui.heading(t(Key::Delete));
                ui.add_space(8.0);
                ui.strong(&name);
                ui.label(egui::RichText::new(t(Key::TrashHint)).weak());
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if pill_danger(ui, t(Key::Confirm)).clicked() {
                        confirm = true;
                    }
                    if pill(ui, t(Key::Cancel), false).clicked() {
                        cancel = true;
                    }
                });
            });
        if confirm {
            self.delete_item(&id);
            self.delete_ask = None;
        } else if cancel || resp.should_close() {
            self.delete_ask = None;
        }
    }

    /// Importação (KeePass/Bitwarden/CSV) — senha do arquivo com 👁.
    fn import_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.import_open {
            return;
        }
        let busy = self.job_label.is_some();
        let mut cancel = false;
        let mut run = false;
        let resp = egui::Modal::new(egui::Id::new("import"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(380.0);
                ui.heading(t(Key::Import));
                ui.add_space(8.0);
                // Duas linhas: os três rótulos juntos estouram a largura.
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.import_kind,
                        ImportKind::Kdbx,
                        t(Key::ImportKdbx),
                    );
                    ui.selectable_value(
                        &mut self.import_kind,
                        ImportKind::Bitwarden,
                        t(Key::ImportBitwarden),
                    );
                });
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.import_kind, ImportKind::Csv, t(Key::ImportCsv));
                });
                ui.add_space(10.0);
                egui::Grid::new("import-grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t(Key::ImportFile));
                        ui.horizontal(|ui| {
                            ui.add(
                                line(&mut self.import_path)
                                    .hint_text("C:\\...\\export")
                                    .desired_width(214.0),
                            );
                            #[cfg(windows)]
                            if ui.button("…").clicked() {
                                if let Some(p) = pick_import_file(self.import_kind) {
                                    self.import_path = p;
                                }
                            }
                        });
                        ui.end_row();
                        // CSV é texto claro; kdbx e Bitwarden cifrado
                        // precisam da senha do arquivo.
                        if self.import_kind != ImportKind::Csv {
                            ui.label(t(Key::ImportPassword));
                            ui.horizontal(|ui| {
                                ui.add(
                                    line(&mut self.import_password)
                                        .password(!self.reveal_import)
                                        .desired_width(214.0),
                                );
                                if ghost(ui, if self.reveal_import { "🙈" } else { "👁" })
                                    .on_hover_text(t(Key::Reveal))
                                    .clicked()
                                {
                                    self.reveal_import = !self.reveal_import;
                                }
                            });
                            ui.end_row();
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let ready = !busy && !self.import_path.trim().is_empty();
                    if ui
                        .add_enabled_ui(ready, |ui| pill(ui, t(Key::Import), true))
                        .inner
                        .clicked()
                    {
                        run = true;
                    }
                    if pill(ui, t(Key::Cancel), false).clicked() {
                        cancel = true;
                    }
                });
            });
        if run {
            self.run_import(ctx);
        } else if cancel || resp.should_close() {
            self.import_open = false;
            self.import_path.clear();
            self.import_password.clear();
            self.reveal_import = false;
        }
    }

    /// Relatório da importação (importados / pulados card-identity).
    fn report_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        let Some((imported, skipped)) = self.import_report else {
            return;
        };
        let mut close = false;
        let resp = egui::Modal::new(egui::Id::new("import-report"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(300.0);
                ui.heading(t(Key::ImportReportTitle));
                ui.add_space(8.0);
                ui.label(format!("{imported} {}", t(Key::Imported)));
                if skipped > 0 {
                    ui.label(
                        egui::RichText::new(format!("{skipped} {}", t(Key::SkippedCardIdentity)))
                            .small()
                            .weak(),
                    );
                }
                ui.add_space(12.0);
                if pill(ui, t(Key::Close), true).clicked() {
                    close = true;
                }
            });
        if close || resp.should_close() {
            self.import_report = None;
        }
    }

    /// Gerador de senha/passphrase + medidor de força.
    fn generator_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.gen_open {
            return;
        }
        let mut close = false;
        let mut generate = false;
        let resp = egui::Modal::new(egui::Id::new("generator"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(360.0);
                ui.heading(t(Key::Generator));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.gen_passphrase, false, t(Key::Password));
                    ui.selectable_value(&mut self.gen_passphrase, true, t(Key::Passphrase));
                });
                ui.add_space(8.0);
                if self.gen_passphrase {
                    ui.add(egui::Slider::new(&mut self.gen_pp.words, 3..=12).text(t(Key::Words)));
                    ui.horizontal(|ui| {
                        ui.label(t(Key::Separator));
                        ui.add(line(&mut self.gen_pp.separator).desired_width(56.0));
                    });
                    ui.checkbox(&mut self.gen_pp.capitalize, t(Key::Capitalize));
                    ui.checkbox(&mut self.gen_pp.include_number, t(Key::IncludeNumber));
                } else {
                    ui.add(
                        egui::Slider::new(&mut self.gen_pw.length, 4..=128).text(t(Key::Length)),
                    );
                    ui.checkbox(&mut self.gen_pw.lowercase, t(Key::Lowercase));
                    ui.checkbox(&mut self.gen_pw.uppercase, t(Key::Uppercase));
                    ui.checkbox(&mut self.gen_pw.digits, t(Key::Digits));
                    ui.checkbox(&mut self.gen_pw.symbols, t(Key::Symbols));
                }
                ui.add_space(10.0);
                if pill(ui, t(Key::Generate), true).clicked() {
                    generate = true;
                }
                if !self.gen_result.is_empty() {
                    ui.add_space(10.0);
                    ui.add(
                        line(&mut self.gen_result)
                            .desired_width(320.0)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.horizontal(|ui| {
                        if ghost(ui, t(Key::Copy)).clicked()
                            && clipboard::copy_secret(self.gen_result.clone()).is_ok()
                        {
                            self.copied(t(Key::Generator));
                        }
                        // "Usar" só faz sentido com o form aberto — é ele que
                        // recebe a senha gerada.
                        if self.form_open && ghost(ui, t(Key::Use)).clicked() {
                            self.add_pass = self.gen_result.clone();
                            self.gen_result.clear();
                            close = true;
                        }
                    });
                }
                ui.add_space(12.0);
                if pill(ui, t(Key::Close), false).clicked() {
                    close = true;
                }
            });
        if generate {
            let made = if self.gen_passphrase {
                generator::generate_passphrase(&self.gen_pp)
            } else {
                generator::generate_password(&self.gen_pw)
            };
            match made {
                Ok(s) => {
                    self.gen_result = s;
                    self.err = None;
                }
                Err(e) => self.err = Some(e),
            }
        }
        if close || resp.should_close() {
            self.gen_open = false;
            self.gen_result.clear();
        }
    }

    /// Troca de senha mestra (Argon2id 2× — roda na thread do job).
    fn change_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.change_open {
            return;
        }
        let busy = self.job_label.is_some();
        let mut cancel = false;
        let mut confirm = false;
        let resp = egui::Modal::new(egui::Id::new("change-master"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(360.0);
                ui.heading(t(Key::ChangeMaster));
                ui.add_space(10.0);
                egui::Grid::new("change-grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t(Key::OldPassword));
                        ui.add(line(&mut self.chg_old).password(true).desired_width(220.0));
                        ui.end_row();
                        ui.label(t(Key::NewPassword));
                        ui.add(line(&mut self.chg_new).password(true).desired_width(220.0));
                        ui.end_row();
                        ui.label(t(Key::NewPasswordConfirm));
                        ui.add(
                            line(&mut self.chg_confirm)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let ready = !busy && !self.chg_new.is_empty();
                    if ui
                        .add_enabled_ui(ready, |ui| pill(ui, t(Key::Confirm), true))
                        .inner
                        .clicked()
                    {
                        confirm = true;
                    }
                    if pill(ui, t(Key::Cancel), false).clicked() {
                        cancel = true;
                    }
                });
            });
        if confirm {
            self.change_master(ctx);
        } else if cancel || resp.should_close() {
            self.change_open = false;
            self.chg_old.clear();
            self.chg_new.clear();
            self.chg_confirm.clear();
        }
    }

    /// Anexos do item: listar/salvar os existentes e anexar um novo.
    fn attachments_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        let Some(item_id) = self.attach_item.clone() else {
            return;
        };
        let ready = self.job_label.is_none();
        let item_name = self
            .items
            .iter()
            .find(|i| i.id == item_id)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        // Owned antes do UI: nenhum borrow de self.raw na closure.
        let mut atts: Vec<(String, String, u64)> = Vec::new();
        if let Some(list) = self
            .raw
            .as_ref()
            .and_then(|r| r.get("items"))
            .and_then(|i| i.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|it| it.get("id").and_then(|v| v.as_str()) == Some(item_id.as_str()))
            })
            .and_then(|item| item.get("attachments"))
            .and_then(|a| a.as_array())
        {
            for a in list {
                let id = a
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = a
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("anexo")
                    .to_string();
                let size = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                atts.push((id, name, size));
            }
        }
        let mut close = false;
        let mut add = false;
        let mut save: Option<String> = None;
        let resp = egui::Modal::new(egui::Id::new("attachments"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(380.0);
                ui.heading(format!("{} — {item_name}", t(Key::Attachments)));
                ui.add_space(8.0);
                if atts.is_empty() {
                    ui.label(egui::RichText::new(t(Key::NoAttachments)).weak());
                } else {
                    for (att_id, name, size) in &atts {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            ui.label(
                                egui::RichText::new(human_size(*size as usize))
                                    .small()
                                    .weak(),
                            );
                            if ui
                                .add_enabled_ui(ready, |ui| pill(ui, t(Key::SaveAttachment), false))
                                .inner
                                .clicked()
                            {
                                save = Some(att_id.clone());
                            }
                        });
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                // No Linux o caminho é digitado (sem gtk no AppImage); no
                // Windows o botão abre o diálogo nativo.
                #[cfg(not(windows))]
                ui.horizontal(|ui| {
                    ui.label(t(Key::ImportFile));
                    ui.add(
                        line(&mut self.attach_path)
                            .hint_text("C:\\...\\arquivo")
                            .desired_width(260.0),
                    );
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(ready, |ui| pill(ui, t(Key::AddAttachment), true))
                        .inner
                        .clicked()
                    {
                        add = true;
                    }
                    if pill(ui, t(Key::Close), false).clicked() {
                        close = true;
                    }
                });
            });
        if let Some(att_id) = save {
            self.save_attachment(&item_id, &att_id, ctx);
        } else if add {
            self.add_attachment_from(&item_id, ctx);
        } else if close || resp.should_close() {
            self.attach_item = None;
            self.attach_path.clear();
        }
    }

    /// Lixeira: lista os itens com `deletedAt`, com Restaurar e Excluir de vez.
    ///
    /// O oficial não tem "esvaziar lixeira": o que existe é `deleteForever`
    /// por item (com `confirm()`), então é isso que o lab oferece — sem
    /// inventar ação em massa.
    fn trash_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.trash_open {
            return;
        }
        let rows = self
            .raw
            .as_ref()
            .map(vault::trashed_items)
            .unwrap_or_default();
        let mut close = false;
        let mut restore: Option<String> = None;
        let mut purge: Option<String> = None;
        let mut ask: Option<String> = None;
        let keep_ask = self.purge_ask.clone();
        let resp = egui::Modal::new(egui::Id::new("trash"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(420.0);
                ui.heading(t(Key::Trash));
                ui.add_space(8.0);
                if rows.is_empty() {
                    ui.label(egui::RichText::new(t(Key::TrashEmpty)).weak());
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for row in &rows {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.strong(&row.name);
                                        ui.label(
                                            egui::RichText::new(date_from_ms(row.deleted_at))
                                                .small()
                                                .weak(),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if keep_ask.as_deref() == Some(row.id.as_str()) {
                                                if pill_danger(ui, t(Key::Confirm)).clicked() {
                                                    purge = Some(row.id.clone());
                                                }
                                                if pill(ui, t(Key::Cancel), false).clicked() {
                                                    ask = Some(String::new());
                                                }
                                            } else {
                                                if ghost(ui, t(Key::DeleteForever)).clicked() {
                                                    ask = Some(row.id.clone());
                                                }
                                                if ghost(ui, t(Key::Restore)).clicked() {
                                                    restore = Some(row.id.clone());
                                                }
                                            }
                                        },
                                    );
                                });
                                ui.separator();
                            }
                        });
                }
                ui.add_space(10.0);
                if pill(ui, t(Key::Close), false).clicked() {
                    close = true;
                }
            });
        if let Some(id) = restore {
            self.restore_item(&id);
        } else if let Some(id) = purge {
            self.purge_item(&id);
        } else if let Some(id) = ask {
            // "" = cancelar o "excluir de vez" deste item.
            self.purge_ask = if id.is_empty() { None } else { Some(id) };
        }
        if close || resp.should_close() {
            self.trash_open = false;
            self.purge_ask = None;
        }
    }

    /// Export em claro (JSON/CSV) — aviso forte antes de gravar.
    fn export_modal(&mut self, ctx: &egui::Context, t: &dyn Fn(Key) -> &'static str) {
        if !self.export_open {
            return;
        }
        let busy = self.job_label.is_some();
        let mut cancel = false;
        let mut run = false;
        let resp = egui::Modal::new(egui::Id::new("export"))
            .frame(modal_frame(ctx))
            .show(ctx, |ui| {
                begin_dialog(ui);
                ui.set_width(420.0);
                ui.heading(t(Key::Export));
                ui.add_space(8.0);
                // Aviso FORTE: o export não tem cifra nenhuma.
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(0xC0, 0x3A, 0x3A).gamma_multiply(0.22))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(t(Key::ExportWarning))
                                .color(egui::Color32::from_rgb(0xE0, 0x6C, 0x6C)),
                        );
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.export_kind,
                        ExportKind::Json,
                        t(Key::ExportJson),
                    );
                    ui.selectable_value(&mut self.export_kind, ExportKind::Csv, t(Key::ExportCsv));
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(t(Key::ImportFile));
                    ui.add(
                        line(&mut self.export_path)
                            .hint_text(format!("C:\\...\\cofre.{}", self.export_kind.ext()))
                            .desired_width(230.0),
                    );
                    #[cfg(windows)]
                    if ui.button("…").clicked() {
                        if let Some(p) = pick_save_export(self.export_kind) {
                            self.export_path = p;
                        }
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let ready = !busy && !self.export_path.trim().is_empty();
                    if ui
                        .add_enabled_ui(ready, |ui| pill(ui, t(Key::Export), true))
                        .inner
                        .clicked()
                    {
                        run = true;
                    }
                    if pill(ui, t(Key::Cancel), false).clicked() {
                        cancel = true;
                    }
                });
            });
        if run {
            self.run_export(ctx);
        } else if cancel || resp.should_close() {
            self.export_open = false;
        }
    }
}

impl eframe::App for KeysApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let lang = self.cfg.lang;
        let t = |k: Key| i18n::t(lang, k);

        // Bandeja (Windows): descobre o HWND o quanto antes (a thread do tray
        // precisa dele pra ShowWindow com a janela oculta).
        #[cfg(windows)]
        winctl::discover();

        // "Sair" da bandeja: encerra pelo caminho limpo, com a janela visível.
        if self.quit.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Fechar (X) minimiza pra bandeja — SÓ no Windows, onde há bandeja.
        // (No Linux o X fecha de verdade.) Sem lock ao esconder: o cofre fica
        // destrancado na memória (decisão do lab — PC da própria pessoa).
        #[cfg(windows)]
        if ctx.input(|i| i.viewport().close_requested()) {
            let quitting = self.quit.load(Ordering::Relaxed);
            if !quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                winctl::hide();
            }
        }

        // TOTP visível → repaint pra contagem andar (barato: 2×/s).
        let tem_totp = self.items.iter().any(|i| i.totp.is_some());
        if tem_totp {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        // Job pesado em andamento: drena o que a thread devolveu e mantém o
        // spinner animado (a thread avisa no fim, mas o giro precisa de frames).
        self.poll_jobs();
        if self.job_label.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Keys");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if lab_ui::settings_ui(ui, &mut self.cfg) {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.locked() {
                self.locked_ui(ui, ctx, &t);
            } else {
                self.unlocked_ui(ui, ctx, &t);
            }
        });

        // ── diálogos (egui::Modal: bloqueia o fundo; Esc/clique fora fecham) ──
        // A ordem é a de empilhamento: o form abre o gerador por cima.
        self.form_modal(ctx, &t);
        self.delete_modal(ctx, &t);
        self.import_modal(ctx, &t);
        self.report_modal(ctx, &t);
        self.generator_modal(ctx, &t);
        self.change_modal(ctx, &t);
        self.attachments_modal(ctx, &t);
        self.trash_modal(ctx, &t);
        self.export_modal(ctx, &t);

        // rodapé: progresso do job (card + spinner) e o toast, centro embaixo
        self.footer(ctx, &t);
    }
}

impl KeysApp {
    fn locked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        t: &dyn Fn(Key) -> &'static str,
    ) {
        // Job rodando (Argon2id): destrancar/criar ficam desabilitados.
        let busy = self.job_label.is_some();
        ui.heading(t(Key::Vault));
        ui.add_space(10.0);

        egui::Grid::new("unlock")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(".tkeys");
                ui.horizontal(|ui| {
                    ui.add(
                        line(&mut self.path)
                            .hint_text("C:\\...\\cofre.tkeys")
                            .desired_width(260.0),
                    );
                    #[cfg(windows)]
                    if ui.button("…").clicked() {
                        if let Some(p) = pick_open() {
                            self.path = p;
                            self.quick_ativa = self.path_has_stored_key();
                        }
                    }
                });
                ui.end_row();

                ui.label(t(Key::MasterPassword));
                ui.add(line(&mut self.master).password(true).desired_width(260.0));
                ui.end_row();
            });

        ui.add_space(4.0);
        #[cfg(windows)]
        ui.checkbox(&mut self.quick_unlock, t(Key::QuickUnlock));
        #[cfg(not(windows))]
        let _ = &self.quick_unlock; // campo existe, UI não (stub desligado)
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ready = !busy && !self.path.trim().is_empty() && !self.master.is_empty();
            if ui
                .add_enabled_ui(ready, |ui| pill(ui, t(Key::Unlock), true))
                .inner
                .clicked()
                || (!busy
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !self.master.is_empty()))
            {
                self.unlock(ctx);
            }
            #[cfg(windows)]
            if self.quick_ativa && ghost(ui, t(Key::ForgetKey)).clicked() {
                self.forget_quick_key();
            }
        });

        if let Some(e) = &self.err {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("{}: {e}", t(Key::Error)))
                    .color(egui::Color32::LIGHT_RED),
            );
        }

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(18.0);

        // ── novo cofre ─────────────────────────────────────────────────
        ui.heading(t(Key::NewVault));
        ui.add_space(6.0);
        egui::Grid::new("new")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(".tkeys");
                ui.horizontal(|ui| {
                    ui.add(
                        line(&mut self.path)
                            .hint_text("C:\\...\\novo-cofre.tkeys")
                            .desired_width(260.0),
                    );
                    #[cfg(windows)]
                    if ui.button("…").clicked() {
                        if let Some(p) = pick_save() {
                            self.path = p;
                        }
                    }
                });
                ui.end_row();
                ui.label(t(Key::MasterPassword));
                ui.add(line(&mut self.master).password(true));
                ui.end_row();
                ui.label(format!("{} ✓", t(Key::Password)));
                ui.add(line(&mut self.confirm).password(true));
                ui.end_row();
            });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ready = !busy && !self.path.trim().is_empty() && !self.master.is_empty();
            if ui
                .add_enabled_ui(ready, |ui| pill(ui, t(Key::NewVault), true))
                .inner
                .clicked()
            {
                self.create(ctx);
            }
        });
    }

    fn unlocked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        t: &dyn Fn(Key) -> &'static str,
    ) {
        let skin = Skin::of(ui);
        // Dados owned antes do UI (nenhum borrow de self.raw nas closures).
        let mut rows: Vec<Row> = self
            .items
            .iter()
            .map(|i| Row {
                id: i.id.clone(),
                name: i.name.clone(),
                username: i.username.clone(),
                totp: i.totp.clone(),
                favorite: i.favorite,
            })
            .collect();
        // Ordenação SÓ de renderização (não regrava nada): favoritos primeiro
        // e, dentro do grupo, nome ascendente — o mesmo sort do VaultScreen do
        // oficial (`Number(b.favorite) - Number(a.favorite) || nome`).
        rows.sort_by(|a, b| {
            b.favorite
                .cmp(&a.favorite)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let needle = self.search.trim().to_lowercase();

        ui.horizontal(|ui| {
            ui.strong(&self.path);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill(ui, t(Key::Add), true).clicked() {
                    self.open_add_form();
                }
                if pill(ui, t(Key::Lock), false).clicked() {
                    self.lock();
                }
            });
        });

        ui.add_space(2.0);
        // Ferramentas (linha própria: o cabeçalho já tem path + lock).
        ui.horizontal_wrapped(|ui| {
            if ghost(ui, t(Key::Import)).clicked() {
                self.import_open = true;
                self.reveal_import = false;
            }
            if ghost(ui, t(Key::Export)).clicked() {
                self.export_path.clear();
                self.export_open = true;
            }
            if ghost(ui, t(Key::Generator)).clicked() {
                self.gen_open = true;
            }
            if ghost(ui, t(Key::ChangeMaster)).clicked() {
                self.change_open = true;
            }
            if ghost(ui, t(Key::Trash)).clicked() {
                self.trash_open = true;
                self.purge_ask = None;
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Filtro "só códigos que mudam" (TOTP): toggle ao lado da busca.
            let filtro = format!("{} ⟳", t(Key::Totp));
            let ink = if self.filter_totp {
                skin.accent
            } else {
                skin.muted
            };
            if ghost_ink(ui, &filtro, ink).clicked() {
                self.filter_totp = !self.filter_totp;
            }
            ui.add(
                line(&mut self.search)
                    .hint_text(t(Key::Search))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(6.0);

        if let Some(e) = &self.err {
            ui.label(
                egui::RichText::new(format!("{}: {e}", t(Key::Error)))
                    .color(egui::Color32::LIGHT_RED),
            );
            ui.add_space(6.0);
        }

        let view: Vec<&Row> = rows
            .iter()
            .filter(|r| needle.is_empty() || r.name.to_lowercase().contains(&needle))
            .filter(|r| !self.filter_totp || r.totp.is_some())
            .collect();

        if view.is_empty() {
            ui.label(egui::RichText::new(t(Key::Empty)).weak());
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for row in view {
                    ui.horizontal(|ui| {
                        // ★ agora é BOTÃO: liga/desliga o favorito (o oficial
                        // faz isso pelo editor, que passa no updateItem).
                        let star = if row.favorite { "★" } else { "☆" };
                        let star_ink = if row.favorite {
                            skin.accent
                        } else {
                            skin.muted
                        };
                        if ghost_ink(ui, star, star_ink)
                            .on_hover_text(t(Key::Favorite))
                            .clicked()
                        {
                            self.toggle_favorite(&row.id);
                        }
                        ui.vertical(|ui| {
                            ui.strong(&row.name);
                            if let Some(u) = &row.username {
                                ui.label(egui::RichText::new(u).small().weak());
                            }
                            // TOTP ao vivo: código + contagem (o repaint de
                            // 500ms já está ligado quando há algum).
                            if let Some(secret) = &row.totp {
                                match totp::now(secret) {
                                    Ok(code) => {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(code.code.clone())
                                                    .monospace()
                                                    .strong(),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{}{}",
                                                    code.seconds_remaining,
                                                    t(Key::SecondsShort)
                                                ))
                                                .small()
                                                .weak(),
                                            );
                                            if ghost(ui, t(Key::Totp))
                                                .on_hover_text(t(Key::Copy))
                                                .clicked()
                                            {
                                                // código é efêmero, mas marcar
                                                // exclusão de histórico não custa
                                                if clipboard::copy_secret(code.code).is_ok() {
                                                    self.copied(&row.name);
                                                }
                                            }
                                        });
                                    }
                                    Err(e) => {
                                        ui.label(
                                            egui::RichText::new(format!("TOTP: {e}"))
                                                .small()
                                                .color(egui::Color32::LIGHT_RED),
                                        );
                                    }
                                }
                            }
                            // Ações do item: ghost buttons (transparentes até
                            // o hover), como no local-pdf.
                            ui.horizontal_wrapped(|ui| {
                                if row.username.is_some()
                                    && ghost(ui, t(Key::Username))
                                        .on_hover_text(t(Key::Copy))
                                        .clicked()
                                {
                                    if let Some((user, _, _)) = self
                                        .raw
                                        .as_ref()
                                        .and_then(|r| vault::login_triple(r, &row.id))
                                    {
                                        // usuário não é segredo: cópia simples
                                        if arboard::Clipboard::new()
                                            .and_then(|mut c| c.set_text(user))
                                            .is_ok()
                                        {
                                            self.copied(&row.name);
                                        }
                                    }
                                }
                                if ghost(ui, t(Key::Password))
                                    .on_hover_text(t(Key::Copy))
                                    .clicked()
                                {
                                    if let Some((_, pass, _)) = self
                                        .raw
                                        .as_ref()
                                        .and_then(|r| vault::login_triple(r, &row.id))
                                    {
                                        if clipboard::copy_secret(pass).is_ok() {
                                            self.copied(&row.name);
                                        }
                                    }
                                }
                                if ghost(ui, t(Key::Attachments)).clicked() {
                                    self.attach_item = Some(row.id.clone());
                                    self.attach_path.clear();
                                }
                                if ghost(ui, t(Key::Edit)).clicked() {
                                    self.open_edit_form(&row.id);
                                }
                                if ghost(ui, t(Key::Delete))
                                    .on_hover_text(t(Key::TrashHint))
                                    .clicked()
                                {
                                    // Confirmação: exclusão é reversível só na
                                    // lixeira — clique sem querer não apaga.
                                    self.delete_ask = Some((row.id.clone(), row.name.clone()));
                                }
                            });
                        });
                    });
                    ui.separator();
                }
            });
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}: {}", t(Key::Items), rows.len()))
                        .small()
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    #[cfg(windows)]
                    if self.quick_ativa && ghost(ui, t(Key::ForgetKey)).clicked() {
                        self.forget_quick_key();
                    }
                });
            });
        });
        // ctx fica no parâmetro pra manter a assinatura simétrica às outras telas.
        let _ = ctx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data da lixeira é o único uso de calendário do lab (sem crate de
    /// data): confere epoch, virada de ano e um dia comum.
    #[test]
    fn data_da_lixeira_sai_em_dd_mm_aaaa() {
        assert_eq!(date_from_ms(0), "01/01/1970");
        assert_eq!(date_from_ms(1_704_067_200_000), "01/01/2024");
        assert_eq!(date_from_ms(1_700_000_000_000), "14/11/2023");
        // 29/02 de um ano bissexto
        assert_eq!(date_from_ms(1_709_164_800_000), "29/02/2024");
    }
}
