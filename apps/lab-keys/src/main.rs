//! lab-keys — piloto de referência do LocalKeys em egui/eframe.
//! Onda 3: cofre `.tkeys` REAL (`crypto.rs`/`totp.rs`/gravação atômica/
//! `copy_secret` copiados verbatim do LocalKeys@0.9.0).
//! Onda 4: TOTP ao vivo (código + contagem), **desbloqueio rápido** (chave
//! derivada no cofre do SO via keyring — mesmo desenho do oficial, opt-in),
//! **editar** item e **excluir** (lixeira lógica, `deletedAt`).
//! Onda 5: **bandeja** (Windows): o X esconde pra bandeja; "Sair" no menu
//! fecha de verdade. Os handlers do tray-icon vivem numa thread própria
//! (janela oculta congela o update — ver `winctl.rs`), como no lab-clip.
//!
//! Nota: o Argon2 roda na thread da UI (~300 ms de freeze no destrancar) — o
//! oficial usa command async do Tauri. Aceitável no lab; anotado como achado.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod crypto;
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
            while let Ok(cmd) = rx.recv() {
                match cmd {
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
    // feedback "copiado"
    copied_hint: Option<(String, std::time::Instant)>,
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
            copied_hint: None,
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

    fn copied(&mut self, what: &str) {
        self.copied_hint = Some((what.to_string(), std::time::Instant::now()));
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
                            ui.add(egui::TextEdit::singleline(&mut self.add_pass).password(true));
                            ui.end_row();
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
