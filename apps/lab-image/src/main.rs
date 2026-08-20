//! lab-image — piloto de referência do LocalImage em egui/eframe.
//! Viewer + EXIF + export re-encodado (privacidade: EXIF nunca sobrevive).
//! Núcleo (`img.rs`) portado do LocalImage oficial.
//!
//! Visualização: a image crate decodifica → textura egui; zoom por roda do
//! mouse com âncora no cursor, pan com arrastar, encaixe na janela (tecla
//! 0/F). Setas ←/→ navegam a pasta (mesma sequência do oficial).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod img;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;

const APP_ID: &str = "lab-image";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);

    // "Abrir com" do Windows manda o caminho como primeiro arg.
    // canonicalize garante que o path bate com o que read_dir retorna.
    let initial_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .and_then(|p| p.canonicalize().ok());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Image")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(ImageApp::new(cfg, initial_file)))
        }),
    )
}

/// Trabalho pesado fora da UI (thread de decode).
enum Job {
    List(String),
    Decode(PathBuf),
    Exif(PathBuf),
    /// Export de privacidade: re-encoda girado 90° como PNG ao lado do
    /// original (`nome.lab.png`) — EXIF nunca sobrevive.
    Export(PathBuf),
}

enum Done {
    List(Result<Vec<PathBuf>, String>),
    Decode {
        path: PathBuf,
        result: Result<(egui::ColorImage, Option<img::ImageInfo>), String>,
    },
    Exif(Vec<(String, String)>),
    Export(Result<PathBuf, String>),
}

fn spawn_job(job: Job, tx: Sender<Done>) {
    std::thread::spawn(move || match job {
        Job::List(dir) => {
            let _ = tx.send(Done::List(img::list_dir(&dir)));
        }
        Job::Decode(path) => {
            // Dimensões/tamanho sem decodificar de novo (cabeçalho).
            let info = img::image_info(&path).ok();
            let result = image::open(&path).map(|im| {
                let rgba = im.to_rgba8();
                (
                    egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    ),
                    info,
                )
            });
            let _ = tx.send(Done::Decode { path, result: result.map_err(|e| e.to_string()) });
        }
        Job::Exif(path) => {
            let _ = tx.send(Done::Exif(img::exif_info(&path)));
        }
        Job::Export(src) => {
            let dst = src.with_extension("lab.png");
            let _ = tx.send(Done::Export(img::export(&src, &dst, 90, None, 90).map(|_| dst)));
        }
    });
}

struct ImageApp {
    cfg: Config,
    dir: String,
    files: Vec<PathBuf>,
    idx: Option<usize>,
    /// Última imagem decodificada (path → textura) — trocou de imagem,
    /// trocou de textura (sem cache infinito: RAM manda).
    tex: Option<(PathBuf, egui::TextureHandle)>,
    exif: Option<Vec<(String, String)>>,
    /// Canal geral: List / Exif / Export.
    rx: Option<Receiver<Done>>,
    /// Canal dedicado do Decode — não pode ser sobrescrito pelo Exif.
    decode_rx: Option<Receiver<Done>>,
    status: String,
    // viewport do zoom
    zoom: f32,
    pan: egui::Vec2,
    fit: bool,
    show_exif: bool,
    /// Se o app foi aberto com um arquivo via args, guardamos o path até
    /// a lista da pasta estar pronta (decode é async via channel).
    initial_file: Option<PathBuf>,
}

impl ImageApp {
    fn new(cfg: Config, initial_file: Option<PathBuf>) -> Self {
        Self {
            cfg,
            dir: String::new(),
            files: Vec::new(),
            idx: None,
            tex: None,
            exif: None,
            rx: None,
            decode_rx: None,
            status: String::new(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit: true,
            show_exif: false,
            initial_file,
        }
    }

    /// Enfileira um job nos canais genéricos (List / Exif / Export).
    fn request(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.rx = Some(rx);
    }

    /// Enfileira um decode — canal dedicado para não ser sobrescrito.
    fn request_decode(&mut self, job: Job) {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_job(job, tx);
        self.decode_rx = Some(rx);
    }

    fn current(&self) -> Option<&PathBuf> {
        self.idx.map(|i| &self.files[i])
    }

    fn goto(&mut self, i: usize) {
        if i < self.files.len() {
            self.idx = Some(i);
            self.tex = None;
            self.exif = None;
            self.fit = true;
            if let Some(p) = self.current() {
                let p = p.clone();
                self.request_decode(Job::Decode(p.clone()));
                self.request(Job::Exif(p));
            }
        }
    }
}

impl eframe::App for ImageApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Se abriu com um arquivo via args, dispara o scan da pasta agora.
        if let Some(ref p) = self.initial_file {
            if self.dir.is_empty() {
                if let Some(parent) = p.parent() {
                    let dir = parent.to_string_lossy().to_string();
                    self.dir = dir.clone();
                    self.request(Job::List(dir));
                    ctx.request_repaint();
                    return;
                }
            }
        }

        // Drena resultados das threads; `goto(0)` fica fora do borrow.
        let mut open_first = false;
        if let Some(rx) = &self.rx {
            while let Ok(done) = rx.try_recv() {
                match done {
                    Done::List(Ok(files)) => {
                        self.files = files;
                        self.status.clear();
                        if !self.files.is_empty() {
                            open_first = true;
                        }
                    }
                    Done::List(Err(e)) => self.status = format!("⚠ {e}"),
                    Done::Exif(entries) => self.exif = Some(entries),
                    Done::Export(Ok(dst)) => {
                        self.status = format!("✓ {}", dst.display());
                    }
                    Done::Export(Err(e)) => self.status = format!("⚠ {e}"),
                    _ => {}
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.rx = None;
            }
        }
        // Canal dedicado do decode (não compartilhado com Exif).
        if let Some(rx) = &self.decode_rx {
            while let Ok(done) = rx.try_recv() {
                if let Done::Decode { path, result } = done {
                    match result {
                        Ok((image, info)) => {
                            if let Some(i) = info {
                                self.status = format!(
                                    "{}×{} · {:.1} MB",
                                    i.width,
                                    i.height,
                                    i.size_bytes as f64 / 1e6
                                );
                            }
                            let tex = ctx.load_texture(
                                "view",
                                image,
                                egui::TextureOptions::default(),
                            );
                            self.tex = Some((path, tex));
                        }
                        Err(e) => self.status = format!("⚠ {e}"),
                    }
                }
            }
            if matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.decode_rx = None;
            }
        }
        if open_first {
            if let Some(ref target) = self.initial_file {
                if let Some(i) = self.files.iter().position(|f| f == target) {
                    self.goto(i);
                } else {
                    self.goto(0);
                }
            } else {
                self.goto(0);
            }
            self.initial_file = None;
        }
        if self.rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }

        // Atalhos de navegação (mesmos do oficial).
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if let Some(i) = self.idx {
                if i > 0 {
                    self.goto(i - 1);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if let Some(i) = self.idx {
                if i + 1 < self.files.len() {
                    self.goto(i + 1);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.show_exif {
            self.show_exif = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            self.fit = true;
        }

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Image");
                let n = self
                    .idx
                    .map(|i| format!("{}/{}", i + 1, self.files.len()))
                    .unwrap_or_default();
                ui.label(egui::RichText::new(n).small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("i")
                        .on_hover_text("EXIF")
                        .clicked()
                    {
                        self.show_exif = !self.show_exif;
                    }
                    if self.current().is_some()
                        && ui
                            .button("⤴")
                            .on_hover_text("export girado (sem EXIF)")
                            .clicked()
                    {
                        if let Some(p) = self.current() {
                            let p = p.clone();
                            self.request(Job::Export(p));
                        }
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.dir)
                        .hint_text(r"C:\...\fotos")
                        .desired_width(f32::INFINITY),
                );
                if ui.button(i18n::t(self.cfg.lang, Key::Open)).clicked() {
                    let d = self.dir.trim().to_string();
                    if !d.is_empty() {
                        self.request(Job::List(d));
                    }
                }
            });
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).small().weak());
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("abra uma pasta")
                            .weak(),
                    );
                });
                return;
            }

            let screen = ui.available_size();
            // Painel EXIF é uma Window ancorada (clip/scroll grátis); aqui
            // só reservamos o espaço pra ela não cobrir a imagem.
            let exif_w = if self.show_exif { 240.0 } else { 0.0 };
            let avail = egui::vec2(screen.x - exif_w, screen.y);

            if let Some((_, tex)) = &self.tex {
                let size = tex.size_vec2();
                let (base_scale, base_off) = if self.fit {
                    let s = (avail.x / size.x).min(avail.y / size.y).min(1.0);
                    let scaled = size * s;
                    (
                        s,
                        egui::vec2(
                            (avail.x - scaled.x) / 2.0,
                            (avail.y - scaled.y) / 2.0,
                        ),
                    )
                } else {
                    (1.0, egui::Vec2::ZERO)
                };

                let response = ui
                    .allocate_response(avail, egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab);
                let rect = response.rect;

                // Zoom pela roda, ancorado no cursor (o "natural").
                let scroll = response.hovered() && ui.input(|i| i.raw_scroll_delta.y.abs() > 0.0);
                if scroll {
                    let factor = if ui.input(|i| i.raw_scroll_delta.y > 0.0) {
                        1.1
                    } else {
                        1.0 / 1.1
                    };
                    self.zoom = (self.zoom * factor).clamp(0.05, 40.0);
                    self.fit = false;
                }

                // Pan com arrastar.
                if response.dragged() {
                    self.pan += response.drag_delta();
                    self.fit = false;
                }
                if response.double_clicked() {
                    self.fit = true;
                    self.pan = egui::Vec2::ZERO;
                }

                let scale = base_scale * self.zoom;
                let dest = egui::Rect::from_min_size(
                    rect.min + base_off + self.pan,
                    size * scale,
                );
                let painter = ui.painter_at(rect);
                painter.image(
                    tex.id(),
                    dest,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            }
        });

        if self.show_exif {
            egui::Window::new("EXIF")
                .id(egui::Id::new("exif"))
                .default_open(true)
                .collapsible(true)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
                .show(ctx, |ui| {
                    match &self.exif {
                        Some(entries) if entries.is_empty() => {
                            ui.label(egui::RichText::new("sem EXIF").weak());
                        }
                        Some(entries) => {
                            egui::Grid::new("exif")
                                .num_columns(2)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    for (k, v) in entries {
                                        ui.label(
                                            egui::RichText::new(k).small().weak(),
                                        );
                                        ui.label(v);
                                        ui.end_row();
                                    }
                                });
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                });
        }
    }
}
