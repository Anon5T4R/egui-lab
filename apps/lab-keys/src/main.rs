//! lab-keys — piloto de referência do LocalKeys em egui/eframe.
//! O teste desta onda é REUSO de back-end: `crypto.rs` e a gravação atômica são
//! copiados verbatim do LocalKeys — um cofre `.tkeys` REAL abre aqui. Escopo:
//! criar/abrir cofre, listar, buscar, copiar senha (com exclusão de histórico +
//! limpeza em 30 s), acrescentar login e trancar (por botão ou ao minimizar —
//! regra do oficial). Sem: gerador, favoritos editáveis, TOTP, troca de senha.
//!
//! Nota: o Argon2 roda na thread da UI (~300 ms de freeze no destrancar) — o
//! oficial usa command async do Tauri. Aceitável no lab; anotado como achado.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod crypto;
mod vault;

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use zeroize::Zeroizing;

const APP_ID: &str = "lab-keys";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Keys")
            .with_inner_size([460.0, 620.0]),
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

struct KeysApp {
    cfg: Config,
    // trancado
    path: String,
    master: String,
    confirm: String,
    err: Option<String>,
    // destrancado
    session: Option<crypto::SessionKey>,
    raw: Option<serde_json::Value>,
    items: Vec<vault::ItemView>,
    search: String,
    // formulário de adição
    add_open: bool,
    add_name: String,
    add_user: String,
    add_pass: String,
    // feedback "copiado"
    copied_hint: Option<(String, std::time::Instant)>,
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

impl KeysApp {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            path: String::new(),
            master: String::new(),
            confirm: String::new(),
            err: None,
            session: None,
            raw: None,
            items: Vec::new(),
            search: String::new(),
            add_open: false,
            add_name: String::new(),
            add_user: String::new(),
            add_pass: String::new(),
            copied_hint: None,
        }
    }

    fn locked(&self) -> bool {
        self.session.is_none()
    }

    /// Tranca: apaga a sessão (Drop → Zeroizing limpa a chave) e o vault em
    /// claro da memória. Orem session → raw, como no oficial.
    fn lock(&mut self) {
        self.session = None;
        self.raw = None;
        self.items.clear();
        self.master.clear();
        self.confirm.clear();
        self.add_pass.clear();
    }

    fn adopt(&mut self, session: crypto::SessionKey, raw: serde_json::Value) {
        self.items = vault::items_view(&raw);
        self.raw = Some(raw);
        self.session = Some(session);
        self.err = None;
        self.master.clear();
        self.confirm.clear();
    }

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
                    Ok(raw) => self.adopt(session, raw),
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
                        self.adopt(session, raw);
                    }
                    Err(e) => self.err = Some(e),
                }
            }
            Err(e) => self.err = Some(e.to_string()),
        }
    }

    /// Acrescenta o login do formulário e salva (seal com nonce novo + write
    /// atômico). Se o salvamento falhar, o item NÃO fica só na memória: o raw
    /// só é atualizado depois de gravar com sucesso — memória e disco nunca
    /// divergem.
    fn add_and_save(&mut self) {
        let name = self.add_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let (user, pass) = (self.add_user.clone(), self.add_pass.clone());

        let raw = match self.raw.as_ref() {
            Some(r) => r.clone(), // cópia de trabalho; descartada se o save falhar
            None => return,
        };
        let mut next = raw;
        vault::add_login(&mut next, &name, &user, &pass);

        let bytes = match serde_json::to_vec(&next) {
            Ok(b) => b,
            Err(e) => {
                self.err = Some(e.to_string());
                return;
            }
        };
        let sealed = match self.session.as_ref().map(|s| s.seal(&bytes)) {
            Some(Ok(file)) => file,
            Some(Err(e)) => {
                self.err = Some(e.to_string());
                return;
            }
            None => return,
        };
        match vault::atomic_write(std::path::Path::new(self.path.trim()), &sealed) {
            Ok(()) => {
                self.add_name.clear();
                self.add_user.clear();
                self.add_pass.clear();
                self.add_open = false;
                self.err = None;
                self.items = vault::items_view(&next);
                self.raw = Some(next);
            }
            Err(e) => self.err = Some(e),
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

        // Regra do oficial: trancar ao ocultar/minimizar a janela.
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            self.lock();
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
                self.unlocked_ui(ui, &t);
            }
        });

        // formulário de adição (janela sobre a lista)
        if self.add_open {
            let mut open = self.add_open;
            egui::Window::new(t(Key::Add))
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
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_pass).password(true),
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
                            self.add_and_save();
                        }
                        if ui.button(t(Key::Cancel)).clicked() {
                            self.add_open = false;
                            self.add_pass.clear();
                        }
                    });
                });
            self.add_open = open;
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

        ui.add_space(8.0);
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
        });

        if let Some(e) = &self.err {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("{}: {e}", t(Key::Error))).color(egui::Color32::LIGHT_RED));
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

    fn unlocked_ui(&mut self, ui: &mut egui::Ui, t: &dyn Fn(Key) -> &'static str) {
        // Dados owned antes do UI (nenhum borrow de self.raw atravessa closures).
        let rows: Vec<(String, String, Option<String>, bool)> = self
            .items
            .iter()
            .map(|i| (i.id.clone(), i.name.clone(), i.username.clone(), i.favorite))
            .collect();
        let needle = self.search.trim().to_lowercase();

        ui.horizontal(|ui| {
            ui.strong(&self.path);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t(Key::Lock)).clicked() {
                    self.lock();
                }
                if ui.button(t(Key::Add)).clicked() {
                    self.add_open = true;
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
            ui.label(egui::RichText::new(format!("{}: {e}", t(Key::Error))).color(egui::Color32::LIGHT_RED));
            ui.add_space(6.0);
        }

        let view: Vec<&(String, String, Option<String>, bool)> = rows
            .iter()
            .filter(|(_, name, _, _)| {
                needle.is_empty() || name.to_lowercase().contains(&needle)
            })
            .collect();

        if view.is_empty() {
            ui.label(egui::RichText::new(t(Key::Empty)).weak());
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (id, name, username, favorite) in view {
                    ui.horizontal(|ui| {
                        if *favorite {
                            ui.label("★");
                        }
                        ui.vertical(|ui| {
                            ui.strong(name);
                            if let Some(u) = username {
                                ui.label(egui::RichText::new(u).small().weak());
                            }
                        });
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .small_button(t(Key::Password))
                                    .on_hover_text(t(Key::Copy))
                                    .clicked()
                                {
                                    if let Some((_, pass)) = self
                                        .raw
                                        .as_ref()
                                        .and_then(|r| vault::login_pair(r, id))
                                    {
                                        if clipboard::copy_secret(pass).is_ok() {
                                            self.copied(name);
                                        }
                                    }
                                }
                                if ui
                                    .small_button(t(Key::Username))
                                    .on_hover_text(t(Key::Copy))
                                    .clicked()
                                {
                                    if let Some((user, _)) = self
                                        .raw
                                        .as_ref()
                                        .and_then(|r| vault::login_pair(r, id))
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
                            },
                        );
                    });
                    ui.separator();
                }
            });
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.label(
                egui::RichText::new(format!("{}: {}", t(Key::Items), rows.len()))
                    .small()
                    .weak(),
            );
        });
    }
}
