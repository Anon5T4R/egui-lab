//! lab-hub — o "TaylorHub do lab": catálogo dos pilotos egui, download das
//! releases do monorepo, instalação em `%LOCALAPPDATA%\LabSuite\<app>\` e
//! atalhos com os ícones REAIS dos irmãos Tauri (baixados dos repos da
//! suíte). Os 32 apps da suíte seguem no TaylorHub; este cuida só do lab.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod install;
mod shortcut;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;

use install::{AppDef, InstalledApp};

const APP_ID: &str = "lab-hub";
const RELEASES_PAGE: &str = "https://github.com/Anon5T4R/egui-lab/releases/latest";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Hub")
            .with_inner_size([520.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(HubApp::new(cfg)))
        }),
    )
}

/// Mensagens das threads de trabalho pra UI.
enum Msg {
    /// Tag da última release (None = falhou; status mostra o motivo).
    Latest(Result<String, String>),
    Progress(f32),
    /// Fim da instalação: Ok(app id) ou Err(motivo).
    Done(Result<String, String>),
}

struct HubApp {
    cfg: Config,
    installed: HashMap<String, InstalledApp>,
    latest: Option<String>,
    latest_err: Option<String>,
    /// Instalação em andamento: (app, canal).
    busy: Option<&'static str>,
    rx: Option<Receiver<Msg>>,
    progress: f32,
    status: String,
    textures: HashMap<String, egui::TextureHandle>,
}

fn spawn_latest(tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let _ = tx.send(Msg::Latest(install::fetch_latest_tag()));
    });
}

fn spawn_install(app: &'static AppDef, tag: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let (itx, irx) = std::sync::mpsc::channel::<f32>();
        // Repassa o progresso de download pro canal da UI (clone: o `tx`
        // original segue na mão desta thread pro Done).
        let tx_fwd = tx.clone();
        std::thread::spawn(move || {
            for p in irx {
                let _ = tx_fwd.send(Msg::Progress(p));
            }
        });
        let mut map = install::load_installed();
        let res = install::install_app(app, &tag, &mut map, &itx).map(|_| app.id.to_string());
        let _ = tx.send(Msg::Done(res));
    });
}

impl HubApp {
    fn new(cfg: Config) -> Self {
        // Carrega os ícones locais (de instalações anteriores) como texturas
        // — o download em si acontece na instalação.
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_latest(tx);
        Self {
            cfg,
            installed: install::load_installed(),
            latest: None,
            latest_err: None,
            busy: None,
            rx: Some(rx),
            progress: 0.0,
            status: String::new(),
            textures: HashMap::new(),
        }
    }

    fn ensure_icon(&mut self, ctx: &egui::Context, app: &AppDef) {
        if self.textures.contains_key(app.id) {
            return;
        }
        let path = install::icon_path(app.id);
        let Some(bytes) = std::fs::read(&path).ok() else {
            return;
        };
        if let Ok(img) = image::load_from_memory(&bytes) {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            self.textures.insert(
                app.id.to_string(),
                ctx.load_texture(app.id, color, egui::TextureOptions::default()),
            );
        }
    }
}

impl eframe::App for HubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drena as mensagens das threads (se houver alguma em voo).
        let mut latest_req = false;
        let mut install_req: Option<&'static AppDef> = None;
        // self.rx é emprestado no drain — desligá-lo é postergado pra fora.
        let mut drop_rx = false;
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    Msg::Latest(Ok(tag)) => {
                        self.latest = Some(tag);
                        self.latest_err = None;
                    }
                    Msg::Latest(Err(e)) => self.latest_err = Some(e),
                    Msg::Progress(p) => self.progress = p,
                    Msg::Done(Ok(id)) => {
                        self.installed = install::load_installed();
                        self.busy = None;
                        drop_rx = true;
                        self.status = format!("{id} ✓");
                    }
                    Msg::Done(Err(e)) => {
                        self.busy = None;
                        drop_rx = true;
                        self.status = format!("⚠ {e}");
                    }
                }
            }
            // Canal só de latest: fecha sozinho quando a thread morre
            // (matches! em vez de ==: TryRecvError não é PartialEq).
            if self.busy.is_none()
                && matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected))
            {
                drop_rx = true;
            }
        }
        if drop_rx {
            self.rx = None;
        }
        if self.rx.is_some() || self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Hub");
                if let Some(t) = &self.latest {
                    ui.label(egui::RichText::new(t).small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("↻").on_hover_text(i18n::t(self.cfg.lang, Key::Refresh)).clicked() {
                        latest_req = true;
                    }
                    if lab_ui::settings_ui(ui, &mut self.cfg) {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("rodape").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("⬇").on_hover_text(RELEASES_PAGE).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(RELEASES_PAGE));
                }
                ui.label(
                    egui::RichText::new(install::install_root().display().to_string())
                        .small()
                        .weak(),
                );
            });
            if self.busy.is_some() {
                let t = i18n::t(self.cfg.lang, Key::Downloading);
                ui.label(format!("{t}… {:.0}%", self.progress * 100.0));
                ui.add(
                    egui::ProgressBar::new(self.progress).show_percentage(),
                );
            } else if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).small().weak());
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let lang = self.cfg.lang;
            let t = |k: Key| i18n::t(lang, k);

            if let Some(e) = &self.latest_err {
                ui.label(
                    egui::RichText::new(format!("{}: {e}", t(Key::Error)))
                        .color(egui::Color32::LIGHT_RED),
                );
                ui.add_space(6.0);
            }

            for app in install::APPS {
                self.ensure_icon(ctx, app);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Ícone real (se já baixado) ou placeholder.
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                        if let Some(tex) = self.textures.get(app.id) {
                            ui.put(rect, egui::Image::from_texture(tex).fit_to_exact_size(rect.size()));
                        } else {
                            ui.painter()
                                .circle_filled(rect.center(), 16.0, ui.style().visuals.weak_text_color());
                        }

                        ui.vertical(|ui| {
                            ui.strong(app.display);
                            let ins = self.installed.get(app.id);
                            match (ins, &self.latest) {
                                (Some(i), Some(l)) if &i.version == l => {
                                    ui.label(egui::RichText::new(format!(
                                        "{} {} · {}",
                                        t(Key::Installed),
                                        i.version,
                                        t(Key::UpToDate)
                                    ))
                                    .small()
                                    .weak());
                                }
                                (Some(i), Some(l)) => {
                                    ui.label(egui::RichText::new(format!(
                                        "{} {} · {} {}",
                                        t(Key::Installed),
                                        i.version,
                                        t(Key::Available),
                                        l
                                    ))
                                    .small()
                                    .color(ui.style().visuals.warn_fg_color));
                                }
                                (Some(i), None) => {
                                    ui.label(egui::RichText::new(format!(
                                        "{} {}",
                                        t(Key::Installed),
                                        i.version
                                    ))
                                    .small()
                                    .weak());
                                }
                                (None, _) => {
                                    ui.label(egui::RichText::new(t(Key::NotInstalled)).small().weak());
                                }
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let busy = self.busy.is_some();
                            // Ordem RightToLeft: o 1º adicionado fica mais à direita.
                            let label = if self.installed.contains_key(app.id) {
                                t(Key::Update)
                            } else {
                                t(Key::Install)
                            };
                            let can_install = !busy && self.latest.is_some() && self.busy != Some(app.id);
                            if ui
                                .add_enabled(can_install, egui::Button::new(label))
                                .clicked()
                            {
                                install_req = Some(app);
                            }
                            if let Some(ins) = self.installed.get(app.id) {
                                if ui
                                    .add_enabled(!busy, egui::Button::new(t(Key::Open)).small())
                                    .on_hover_text(&ins.exe)
                                    .clicked()
                                {
                                    let _ = std::process::Command::new(&ins.exe).spawn();
                                }
                                let exe = std::path::PathBuf::from(&ins.exe);
                                let icon = install::icon_path(app.id);
                                if ui
                                    .add_enabled(!busy, egui::Button::new(t(Key::StartMenu)).small())
                                    .clicked()
                                {
                                    match shortcut::create(app.id, app.display, &exe, &icon, shortcut::Where::StartMenu) {
                                        Ok(p) => self.status = format!("✓ {}", p.display()),
                                        Err(e) => self.status = format!("⚠ {e}"),
                                    }
                                }
                                if ui
                                    .add_enabled(!busy, egui::Button::new(t(Key::Desktop)).small())
                                    .clicked()
                                {
                                    match shortcut::create(app.id, app.display, &exe, &icon, shortcut::Where::Desktop) {
                                        Ok(p) => self.status = format!("✓ {}", p.display()),
                                        Err(e) => self.status = format!("⚠ {e}"),
                                    }
                                }
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
        });

        // Ações pedidas pela UI deste frame (depois dos borrows fecharem).
        if latest_req {
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_latest(tx);
            self.rx = Some(rx);
            self.status.clear();
        }
        if let Some(app) = install_req {
            let tag = self.latest.clone().unwrap_or_default();
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_install(app, tag, tx);
            self.rx = Some(rx);
            self.busy = Some(app.id);
            self.progress = 0.0;
            self.status.clear();
        }
    }
}
