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
//! força, anexos, troca de senha mestra e auto-type (Windows).
//!
//! Nota: o Argon2 roda na thread da UI (~300 ms de freeze no destrancar) — o
//! oficial usa command async do Tauri. Aceitável no lab; anotado como achado.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autotype;
mod bwdecrypt;
mod clipboard;
mod crypto;
mod generator;
mod import;
mod kdbx;
mod totp;
mod vault;

#[cfg(windows)]
mod winctl;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;
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

/// Export cifrado do Bitwarden traz `passwordProtected` (ou `encrypted`) true.
/// Checamos ANTES de decifrar pra não chamar o decrypt num JSON claro.
fn bitwarden_encrypted(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|v| {
            v.get("passwordProtected").and_then(serde_json::Value::as_bool) == Some(true)
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

/// Troca a master password de um vault FECHADO no disco — mesmo fluxo do
/// `change_master_password` do oficial: decifra com a antiga, recifra com a
/// nova (salt/nonce novos) e grava atômico. A chave do desbloqueio rápido no
/// keyring foi derivada do salt antigo, então a entrada é invalidada.
fn change_master_password(path: &str, old_password: &str, new_password: &str) -> Result<(), String> {
    if new_password.is_empty() {
        return Err("a nova master password não pode ser vazia".into());
    }
    let file = std::fs::read(path).map_err(|e| format!("falha ao ler '{path}': {e}"))?;
    let renewed = crypto::change_password(old_password, new_password, &file)
        .map_err(|e| e.to_string())?;
    vault::atomic_write(std::path::Path::new(path), &renewed)?;
    #[cfg(windows)]
    {
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, path) {
            let _ = entry.delete_credential();
        }
    }
    Ok(())
}

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
    // formulário de adição/edição
    form_open: bool,
    form_edit_id: Option<String>,
    add_name: String,
    add_user: String,
    add_pass: String,
    add_totp: String,
    /// Exclusão esperando confirmação (um clique sem querer não apaga).
    delete_ask: Option<(String, String)>,
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
    // feedback "copiado"
    copied_hint: Option<(String, std::time::Instant)>,
    /// Feedback de operações que não são "copiado" (ex.: senha mestra trocada).
    notice_hint: Option<(String, std::time::Instant)>,
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
            form_open: false,
            form_edit_id: None,
            add_name: String::new(),
            add_user: String::new(),
            add_pass: String::new(),
            add_totp: String::new(),
            delete_ask: None,
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
            copied_hint: None,
            notice_hint: None,
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
        self.delete_ask = None;
        // Onda 6: nenhum segredo de import/gerador/senha mestra sobrevive ao
        // trancar, e as janelas fecham junto.
        self.import_open = false;
        self.import_path.clear();
        self.import_password.clear();
        self.import_report = None;
        self.gen_open = false;
        self.gen_result.clear();
        self.change_open = false;
        self.chg_old.clear();
        self.chg_new.clear();
        self.chg_confirm.clear();
        self.attach_item = None;
        self.attach_path.clear();
        self.notice_hint = None;
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

    fn unlock(&mut self) {
        let path = self.path.trim().to_string();
        if path.is_empty() || self.master.is_empty() {
            return;
        }
        let pw = Zeroizing::new(std::mem::take(&mut self.master));
        let file = match std::fs::read(&path) {
            Ok(f) => f,
            Err(e) => {
                self.err = Some(format!("{path}: {e}"));
                return;
            }
        };
        match crypto::open_vault(&pw, &file) {
            Ok((plaintext, session)) => {
                match serde_json::from_slice::<serde_json::Value>(&plaintext) {
                    Ok(raw) => {
                        if self.quick_unlock {
                            self.store_quick_key(&session);
                        }
                        self.adopt(session, raw);
                        remember_last_path(&path);
                        self.quick_ativa = self.path_has_stored_key();
                    }
                    Err(_) => self.err = Some("vault não é JSON válido".into()),
                }
            }
            Err(crypto::CryptoError::Decrypt) => {
                self.err = Some(i18n::t(self.cfg.lang, Key::WrongPassword).into());
            }
            Err(e) => self.err = Some(e.to_string()),
        }
    }

    fn create(&mut self) {
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
        let pw = Zeroizing::new(std::mem::take(&mut self.master));
        self.confirm.clear();
        match crypto::create_vault(&pw, vault::EMPTY_VAULT.as_bytes()) {
            Ok((file, session)) => {
                let path = std::path::PathBuf::from(self.path.trim());
                match vault::atomic_write(&path, &file) {
                    Ok(()) => {
                        let raw = serde_json::from_str(vault::EMPTY_VAULT)
                            .expect("EMPTY_VAULT é JSON válido");
                        if self.quick_unlock {
                            self.store_quick_key(&session);
                        }
                        self.adopt(session, raw);
                        remember_last_path(&self.path.trim());
                    }
                    Err(e) => self.err = Some(e),
                }
            }
            Err(e) => self.err = Some(e.to_string()),
        }
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
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        let ok = match self.form_edit_id.clone() {
            Some(id) => vault::edit_login(
                &mut next,
                &id,
                &name,
                &self.add_user,
                &self.add_pass,
                self.add_totp.trim(),
            ),
            None => {
                vault::add_login(&mut next, &name, &self.add_user, &self.add_pass.clone());
                // add_login não tem campo totp — insere no item recém-criado
                if !self.add_totp.trim().is_empty() {
                    let n = next["items"].as_array().map(Vec::len).unwrap_or(0);
                    if n > 0 {
                        next["items"][n - 1]["login"]["totp"] =
                            serde_json::json!(self.add_totp.trim());
                    }
                }
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
            self.form_open = false;
            self.form_edit_id = None;
        }
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

    // ── onda 6: importação ──────────────────────────────────────────────

    /// Lê o arquivo escolhido, converte pro schema do vault e persiste. O
    /// relatório (importados/pulados) abre numa janela própria.
    fn run_import(&mut self) {
        let path = self.import_path.trim().to_string();
        if path.is_empty() {
            self.err = Some("informe o arquivo a importar".into());
            return;
        }
        let password = self.import_password.clone();
        let result: Result<(Vec<serde_json::Value>, usize), String> = match self.import_kind {
            ImportKind::Kdbx => {
                kdbx::import(&path, &password).map(|entries| (import::kdbx_to_items(&entries), 0))
            }
            ImportKind::Bitwarden => (|| -> Result<(Vec<serde_json::Value>, usize), String> {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("falha ao ler '{path}': {e}"))?;
                // Export cifrado: decifra ANTES de mapear (o parse espera o
                // mesmo JSON claro do export não-cifrado).
                let clear = if bitwarden_encrypted(&text) {
                    bwdecrypt::decrypt_export(&text, &password)?
                } else {
                    text
                };
                let report = import::parse_bitwarden_json(&clear)?;
                Ok((report.items, report.skipped_card_identity))
            })(),
            ImportKind::Csv => std::fs::read_to_string(&path)
                .map(|text| (import::parse_csv_items(&text), 0))
                .map_err(|e| format!("falha ao ler '{path}': {e}")),
        };
        let (items, skipped) = match result {
            Ok(v) => v,
            Err(e) => {
                self.err = Some(e);
                return;
            }
        };

        let Some(mut next) = self.raw.clone() else {
            return;
        };
        let imported = items.len();
        vault::push_items(&mut next, items);
        if self.persist(next) {
            self.import_report = Some((imported, skipped));
            self.import_open = false;
            self.import_path.clear();
            self.import_password.clear();
        }
    }

    // ── onda 6: troca de senha mestra ───────────────────────────────────

    fn change_master(&mut self) {
        if self.chg_new.is_empty() {
            self.err = Some("a nova senha mestra não pode ser vazia".into());
            return;
        }
        if self.chg_new != self.chg_confirm {
            self.err = Some("as senhas não conferem".into());
            return;
        }
        match change_master_password(self.path.trim(), &self.chg_old, &self.chg_new) {
            Ok(()) => {
                // A sessão em memória foi derivada do salt ANTIGO: um save
                // reescreveria o arquivo com a senha velha. Trancar força o
                // destrancar com a nova (o "vault fechado no disco" que o
                // oficial assume nessa operação).
                self.lock();
                // A entrada do keyring foi invalidada junto com a senha.
                self.quick_ativa = false;
                self.err = None;
                let msg = i18n::t(self.cfg.lang, Key::MasterChanged).to_string();
                self.notice(msg);
            }
            Err(e) => self.err = Some(e),
        }
    }

    // ── onda 6: anexos ──────────────────────────────────────────────────

    /// Lê o arquivo de origem (picker no Windows; caminho digitado no Linux),
    /// codifica em base64 e anexa ao item. O blob do cofre já cifra tudo.
    fn add_attachment_from(&mut self, item_id: &str) {
        use base64::Engine as _;

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
        let bytes = match std::fs::read(&source) {
            Ok(b) => b,
            Err(e) => {
                self.err = Some(format!("falha ao ler '{source}': {e}"));
                return;
            }
        };
        let name = std::path::Path::new(&source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("anexo")
            .to_string();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mime = mime_by_ext(&source);
        let Some(mut next) = self.raw.clone() else {
            return;
        };
        if vault::add_attachment(&mut next, item_id, &name, mime, &b64) {
            self.persist(next);
        } else {
            self.err = Some("não foi possível anexar ao item".into());
        }
    }

    /// Decodifica o base64 do anexo e grava no destino escolhido.
    fn save_attachment(&mut self, item_id: &str, attachment_id: &str) {
        use base64::Engine as _;

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
        let bytes = match base64::engine::general_purpose::STANDARD.decode(&b64) {
            Ok(b) => b,
            Err(_) => {
                self.err = Some("anexo com base64 inválido".into());
                return;
            }
        };
        match std::fs::write(&target, &bytes) {
            Ok(()) => {
                self.err = None;
                let msg = format!("{}: {target}", i18n::t(self.cfg.lang, Key::SaveAttachment));
                self.notice(msg);
            }
            Err(e) => self.err = Some(format!("falha ao gravar '{target}': {e}")),
        }
    }

    // ── feedback ────────────────────────────────────────────────────────

    fn copied(&mut self, what: &str) {
        self.copied_hint = Some((what.to_string(), std::time::Instant::now()));
    }

    fn notice(&mut self, msg: String) {
        self.notice_hint = Some((msg, std::time::Instant::now()));
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
                self.locked_ui(ui, &t);
            } else {
                self.unlocked_ui(ui, ctx, &t);
            }
        });

        // formulário de adição/edição (janela sobre a lista)
        if self.form_open {
            let mut open = self.form_open;
            let title = if self.form_edit_id.is_some() {
                t(Key::Edit)
            } else {
                t(Key::Add)
            };
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("add")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(t(Key::Name));
                            ui.text_edit_singleline(&mut self.add_name);
                            ui.end_row();
                            ui.label(t(Key::Username));
                            ui.text_edit_singleline(&mut self.add_user);
                            ui.end_row();
                            ui.label(t(Key::Password));
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.add_pass)
                                        .password(true)
                                        .desired_width(200.0),
                                );
                                // Atalho pro gerador (mesma janela da barra).
                                if ui.small_button(t(Key::Generate)).clicked() {
                                    self.gen_open = true;
                                }
                            });
                            ui.end_row();
                            // Força ao vivo (zxcvbn) — só quando há senha.
                            if !self.add_pass.is_empty() {
                                let strength = generator::password_strength(
                                    &self.add_pass,
                                    &[self.add_name.as_str()],
                                );
                                ui.label("");
                                ui.add(
                                    egui::ProgressBar::new(
                                        (f32::from(strength.score) + 1.0) / 5.0,
                                    )
                                    .desired_width(200.0)
                                    .fill(strength_color(strength.score))
                                    .text(t(strength_key(strength.score))),
                                )
                                .on_hover_text(format!(
                                    "zxcvbn: 10^{:.1} tentativas",
                                    strength.guesses_log10
                                ));
                                ui.end_row();
                            }
                            ui.label(t(Key::Totp));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_totp)
                                    .hint_text("base32 (opcional)"),
                            );
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.add_name.trim().is_empty(),
                                egui::Button::new(t(Key::Save)),
                            )
                            .clicked()
                        {
                            self.save_form();
                        }
                        if ui.button(t(Key::Cancel)).clicked() {
                            self.form_open = false;
                            self.form_edit_id = None;
                            self.add_pass.clear();
                        }
                    });
                });
            self.form_open = open;
        }

        // confirmação de exclusão (lixeira — mas não sem querer).
        if let Some((id, name)) = self.delete_ask.clone() {
            let mut open = true;
            egui::Window::new(t(Key::Delete))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.strong(&name);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t(Key::TrashHint)).weak());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t(Key::Confirm)).clicked() {
                            self.delete_item(&id);
                            self.delete_ask = None;
                        }
                        if ui.button(t(Key::Cancel)).clicked() {
                            self.delete_ask = None;
                        }
                    });
                });
            if !open {
                self.delete_ask = None;
            }
        }

        // ── importação (onda 6) ─────────────────────────────────────────
        if self.import_open {
            let mut open = true;
            egui::Window::new(t(Key::Import))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    // Duas linhas: os três rótulos juntos estourariam a
                    // largura da janela (480px) no título comprido do CSV.
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
                        ui.selectable_value(
                            &mut self.import_kind,
                            ImportKind::Csv,
                            t(Key::ImportCsv),
                        );
                    });
                    ui.add_space(6.0);
                    egui::Grid::new("import")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(t(Key::ImportFile));
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.import_path)
                                        .hint_text("C:\\...\\export")
                                        .desired_width(230.0),
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
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.import_password)
                                        .password(true)
                                        .desired_width(230.0),
                                );
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(
                            !self.import_path.trim().is_empty(),
                            egui::Button::new(t(Key::Import)),
                        )
                        .clicked()
                    {
                        self.run_import();
                    }
                });
            if !open {
                self.import_open = false;
                self.import_path.clear();
                self.import_password.clear();
            }
        }

        // relatório da importação
        if let Some((imported, skipped)) = self.import_report {
            let mut open = true;
            egui::Window::new(t(Key::ImportReportTitle))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!("{imported} {}", t(Key::Imported)));
                    if skipped > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{skipped} {}",
                                t(Key::SkippedCardIdentity)
                            ))
                            .small()
                            .weak(),
                        );
                    }
                });
            if !open {
                self.import_report = None;
            }
        }

        // ── gerador (onda 6) ────────────────────────────────────────────
        if self.gen_open {
            let mut open = true;
            egui::Window::new(t(Key::Generator))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.gen_passphrase, false, t(Key::Password));
                        ui.selectable_value(&mut self.gen_passphrase, true, t(Key::Passphrase));
                    });
                    ui.add_space(6.0);
                    if self.gen_passphrase {
                        ui.add(
                            egui::Slider::new(&mut self.gen_pp.words, 3..=12)
                                .text(t(Key::Words)),
                        );
                        ui.horizontal(|ui| {
                            ui.label(t(Key::Separator));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.gen_pp.separator)
                                    .desired_width(48.0),
                            );
                        });
                        ui.checkbox(&mut self.gen_pp.capitalize, t(Key::Capitalize));
                        ui.checkbox(&mut self.gen_pp.include_number, t(Key::IncludeNumber));
                    } else {
                        ui.add(
                            egui::Slider::new(&mut self.gen_pw.length, 4..=128)
                                .text(t(Key::Length)),
                        );
                        ui.checkbox(&mut self.gen_pw.lowercase, t(Key::Lowercase));
                        ui.checkbox(&mut self.gen_pw.uppercase, t(Key::Uppercase));
                        ui.checkbox(&mut self.gen_pw.digits, t(Key::Digits));
                        ui.checkbox(&mut self.gen_pw.symbols, t(Key::Symbols));
                    }
                    ui.add_space(8.0);
                    if ui.button(t(Key::Generate)).clicked() {
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
                    if !self.gen_result.is_empty() {
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.gen_result)
                                .desired_width(300.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.horizontal(|ui| {
                            if ui.small_button(t(Key::Copy)).clicked()
                                && clipboard::copy_secret(self.gen_result.clone()).is_ok()
                            {
                                self.copied(t(Key::Generator));
                            }
                            // "Usar" só faz sentido com o form aberto — é ele
                            // que recebe a senha gerada.
                            if self.form_open && ui.small_button(t(Key::Use)).clicked() {
                                self.add_pass = self.gen_result.clone();
                                self.gen_result.clear();
                                self.gen_open = false;
                            }
                        });
                    }
                });
            if !open {
                self.gen_open = false;
                self.gen_result.clear();
            }
        }

        // ── troca de senha mestra (onda 6) ──────────────────────────────
        if self.change_open {
            let mut open = true;
            egui::Window::new(t(Key::ChangeMaster))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("change-master")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(t(Key::OldPassword));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.chg_old).password(true),
                            );
                            ui.end_row();
                            ui.label(t(Key::NewPassword));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.chg_new).password(true),
                            );
                            ui.end_row();
                            ui.label(t(Key::NewPasswordConfirm));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.chg_confirm).password(true),
                            );
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(
                            !self.chg_new.is_empty(),
                            egui::Button::new(t(Key::Confirm)),
                        )
                        .clicked()
                    {
                        self.change_master();
                    }
                });
            if !open {
                self.change_open = false;
                self.chg_old.clear();
                self.chg_new.clear();
                self.chg_confirm.clear();
            }
        }

        // ── anexos do item (onda 6) ─────────────────────────────────────
        if let Some(item_id) = self.attach_item.clone() {
            let mut open = true;
            let item_name = self
                .items
                .iter()
                .find(|i| i.id == item_id)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            // Dados owned antes do UI: nenhum borrow de self.raw na closure.
            let mut atts: Vec<(String, String, u64)> = Vec::new();
            if let Some(item) = self
                .raw
                .as_ref()
                .and_then(|r| r.get("items"))
                .and_then(|i| i.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|it| it.get("id").and_then(|v| v.as_str()) == Some(item_id.as_str()))
                })
            {
                if let Some(list) = item.get("attachments").and_then(|a| a.as_array()) {
                    for a in list {
                        let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let name = a
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("anexo")
                            .to_string();
                        let size = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                        atts.push((id, name, size));
                    }
                }
            }
            egui::Window::new(format!("{} — {item_name}", t(Key::Attachments)))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
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
                                if ui.small_button(t(Key::SaveAttachment)).clicked() {
                                    self.save_attachment(&item_id, att_id);
                                }
                            });
                        }
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    // No Linux o caminho é digitado (sem gtk no AppImage); no
                    // Windows o botão abre o diálogo nativo.
                    #[cfg(not(windows))]
                    {
                        ui.horizontal(|ui| {
                            ui.label(t(Key::ImportFile));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.attach_path)
                                    .hint_text("C:\\...\\arquivo")
                                    .desired_width(260.0),
                            );
                        });
                    }
                    if ui.button(t(Key::AddAttachment)).clicked() {
                        self.add_attachment_from(&item_id);
                    }
                });
            if !open {
                self.attach_item = None;
                self.attach_path.clear();
            }
        }

        if let Some((what, at)) = &self.copied_hint {
            if at.elapsed() < std::time::Duration::from_secs(2) {
                egui::Window::new("ok")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -8.0])
                    .show(ctx, |ui| {
                        ui.label(format!("✓ {} — {}", t(Key::Copy), what));
                    });
            } else {
                self.copied_hint = None;
            }
        }

        // feedback de operação (não é cópia — ex.: senha mestra trocada)
        if let Some((msg, at)) = &self.notice_hint {
            if at.elapsed() < std::time::Duration::from_secs(3) {
                egui::Window::new("ok-notice")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -40.0])
                    .show(ctx, |ui| {
                        ui.label(format!("✓ {msg}"));
                    });
            } else {
                self.notice_hint = None;
            }
        }
    }
}

impl KeysApp {
    fn locked_ui(&mut self, ui: &mut egui::Ui, t: &dyn Fn(Key) -> &'static str) {
        ui.heading(t(Key::Vault));
        ui.add_space(10.0);

        egui::Grid::new("unlock")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(".tkeys");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.path)
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.master)
                        .password(true)
                        .desired_width(260.0),
                );
                ui.end_row();
            });

        ui.add_space(4.0);
        #[cfg(windows)]
        ui.checkbox(&mut self.quick_unlock, t(Key::QuickUnlock));
        #[cfg(not(windows))]
        let _ = &self.quick_unlock; // campo existe, UI não (stub desligado)
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.path.trim().is_empty() && !self.master.is_empty(),
                    egui::Button::new(t(Key::Unlock)),
                )
                .clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Enter) && !self.master.is_empty())
            {
                self.unlock();
            }
            #[cfg(windows)]
            if self.quick_ativa
                && ui
                    .add_enabled(
                        !self.path.trim().is_empty(),
                        egui::Button::new(t(Key::ForgetKey)).small(),
                    )
                    .clicked()
            {
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
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(".tkeys");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.path)
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
                ui.add(egui::TextEdit::singleline(&mut self.master).password(true));
                ui.end_row();
                ui.label(format!("{} ✓", t(Key::Password)));
                ui.add(egui::TextEdit::singleline(&mut self.confirm).password(true));
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.path.trim().is_empty() && !self.master.is_empty(),
                    egui::Button::new(t(Key::NewVault)),
                )
                .clicked()
            {
                self.create();
            }
        });
    }

    fn unlocked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        t: &dyn Fn(Key) -> &'static str,
    ) {
        // Dados owned antes do UI (nenhum borrow de self.raw atravessa closures).
        let rows: Vec<(String, String, Option<String>, Option<String>, bool)> = self
            .items
            .iter()
            .map(|i| {
                (
                    i.id.clone(),
                    i.name.clone(),
                    i.username.clone(),
                    i.totp.clone(),
                    i.favorite,
                )
            })
            .collect();
        let needle = self.search.trim().to_lowercase();

        ui.horizontal(|ui| {
            ui.strong(&self.path);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t(Key::Lock)).clicked() {
                    self.lock();
                }
                if ui.button(t(Key::Add)).clicked() {
                    self.form_edit_id = None;
                    self.add_name.clear();
                    self.add_user.clear();
                    self.add_pass.clear();
                    self.add_totp.clear();
                    self.form_open = true;
                }
            });
        });

        ui.add_space(2.0);
        // Ferramentas da onda 6 (linha própria: o cabeçalho já tem path+lock).
        ui.horizontal(|ui| {
            if ui.small_button(t(Key::Import)).clicked() {
                self.import_open = true;
            }
            if ui.small_button(t(Key::Generate)).clicked() {
                self.gen_open = true;
            }
            if ui.small_button(t(Key::ChangeMaster)).clicked() {
                self.change_open = true;
            }
        });

        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(t(Key::Search))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);

        if let Some(e) = &self.err {
            ui.label(
                egui::RichText::new(format!("{}: {e}", t(Key::Error)))
                    .color(egui::Color32::LIGHT_RED),
            );
            ui.add_space(6.0);
        }

        let view: Vec<&(String, String, Option<String>, Option<String>, bool)> = rows
            .iter()
            .filter(|(_, name, _, _, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
            .collect();

        if view.is_empty() {
            ui.label(egui::RichText::new(t(Key::Empty)).weak());
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (id, name, username, totp, favorite) in view {
                    ui.horizontal(|ui| {
                        if *favorite {
                            ui.label("★");
                        }
                        ui.vertical(|ui| {
                            ui.strong(name);
                            if let Some(u) = username {
                                ui.label(egui::RichText::new(u).small().weak());
                            }
                            // TOTP ao vivo: código + contagem (o repaint de
                            // 500ms já está ligado quando há algum).
                            if let Some(secret) = totp {
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
                                            if ui
                                                .small_button(t(Key::Totp))
                                                .on_hover_text(t(Key::Copy))
                                                .clicked()
                                            {
                                                // código é efêmero, mas marcar
                                                // exclusão de histórico não custa
                                                if clipboard::copy_secret(code.code).is_ok() {
                                                    self.copied(name);
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
                        });
                        // Ações junto do item (feedback v0.1.1).
                        if username.is_some() {
                            if ui
                                .small_button(t(Key::Username))
                                .on_hover_text(t(Key::Copy))
                                .clicked()
                            {
                                if let Some((user, _, _)) =
                                    self.raw.as_ref().and_then(|r| vault::login_triple(r, id))
                                {
                                    // usuário não é segredo: cópia simples
                                    if arboard::Clipboard::new()
                                        .and_then(|mut c| c.set_text(user.clone()))
                                        .is_ok()
                                    {
                                        self.copied(name);
                                    }
                                }
                            }
                        }
                        if ui
                            .small_button(t(Key::Password))
                            .on_hover_text(t(Key::Copy))
                            .clicked()
                        {
                            if let Some((_, pass, _)) =
                                self.raw.as_ref().and_then(|r| vault::login_triple(r, id))
                            {
                                if clipboard::copy_secret(pass).is_ok() {
                                    self.copied(name);
                                }
                            }
                        }
                        if ui
                            .small_button(t(Key::Attachments))
                            .on_hover_text(t(Key::Attachments))
                            .clicked()
                        {
                            self.attach_item = Some(id.clone());
                            self.attach_path.clear();
                        }
                        // Auto-type: só Windows (no Linux o enigo puxaria X11
                        // dev) e só com usuário, como o botão de copiar user.
                        #[cfg(windows)]
                        if username.is_some() && ui.small_button(t(Key::Autotype)).clicked() {
                            if let Some((user, pass, _)) =
                                self.raw.as_ref().and_then(|r| vault::login_triple(r, id))
                            {
                                match autotype::type_login(&user, &pass) {
                                    Ok(()) => self.err = None,
                                    Err(e) => self.err = Some(e),
                                }
                            }
                        }
                        if ui.small_button(t(Key::Edit)).clicked() {
                            if let Some((user, pass, totp)) =
                                self.raw.as_ref().and_then(|r| vault::login_triple(r, id))
                            {
                                self.form_edit_id = Some(id.clone());
                                self.add_name = name.clone();
                                self.add_user = user;
                                self.add_pass = pass;
                                self.add_totp = totp;
                                self.form_open = true;
                            }
                        }
                        if ui
                            .small_button(t(Key::Delete))
                            .on_hover_text("lixeira")
                            .clicked()
                        {
                            // Confirmação: exclusão é reversível só na lixeira
                            // do oficial — clique sem querer não apaga.
                            self.delete_ask = Some((id.clone(), name.clone()));
                        }
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
                    if self.quick_ativa && ui.small_button(t(Key::ForgetKey)).clicked() {
                        self.forget_quick_key();
                    }
                });
            });
        });
        // ctx usado pelos repaints do TOTP (param à toa se não houver totp)
        let _ = ctx;
    }
}
