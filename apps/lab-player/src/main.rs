//! lab-player — piloto de referência do LocalPlayer em egui/eframe.
//! Controle remoto do mpv (processo separado + JSON IPC, como o oficial):
//! playlist, play/pause, seek, volume e resume. O vídeo roda na janela
//! própria do mpv; esta janela é o remote.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod embed;
mod mpv;
mod mpv_setup;
mod resume;

use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use mpv::{Cmd, Event};

const APP_ID: &str = "lab-player";

/// Extensões aceitas na playlist (o filtro do diálogo, do drag-drop e dos args).
const MEDIA_EXTS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "avi", "m4v", "wmv", "mp3", "flac", "ogg", "wav", "m4a",
    "opus",
];

fn is_media(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .map(|x| MEDIA_EXTS.contains(&x.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);

    // "Abrir com" do Windows manda os caminhos como args (pode ser mais de um).
    let args: Vec<String> = std::env::args().skip(1).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Player")
            .with_inner_size([420.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(PlayerApp::new(cfg, args)))
        }),
    )
}

struct PlayerApp {
    cfg: Config,
    cmd_tx: Sender<Cmd>,
    ev_rx: Receiver<Event>,
    playlist: resume::Playlist,
    idx: Option<usize>,
    /// nome do arquivo atual (display).
    now: String,
    time: f64,
    duration: f64,
    paused: bool,
    volume: f64,
    resume: resume::Resume,
    status: String,
    /// Setup do mpv: download em background se necessário.
    mpv_check: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// Índice a tocar assim que o mpv estiver pronto (veio de args).
    initial_play: Option<usize>,
    /// Child window que recebe o vídeo (`--wid`). Windows-only.
    embed: Option<embed::VideoEmbed>,
}

impl PlayerApp {
    fn new(cfg: Config, args: Vec<String>) -> Self {
        let (cmd_tx, ev_rx) = mpv::spawn();

        // "Abrir com": adiciona os arquivos de args na playlist e marca o
        // primeiro pra tocar assim que o mpv estiver disponível (pode estar
        // baixando ainda).
        let mut playlist = resume::load_playlist();
        let mut first_arg: Option<String> = None;
        for a in &args {
            if !is_media(std::path::Path::new(a)) {
                continue;
            }
            if !playlist.files.contains(a) {
                playlist.files.push(a.clone());
            }
            if first_arg.is_none() {
                first_arg = Some(a.clone());
            }
        }
        let initial_play = first_arg
            .and_then(|f| playlist.files.iter().position(|x| *x == f));
        if initial_play.is_some() {
            resume::save_playlist(&playlist);
        }

        // Checa se o mpv está disponível. Se não, dispara download em background.
        let (mpv_check, status_msg) = match mpv_setup::check() {
            mpv_setup::MpvStatus::Ready => (None, String::new()),
            mpv_setup::MpvStatus::NeedsDownload(msg) => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(mpv_setup::download());
                });
                eprintln!("[lab-player] {msg}");
                (Some(rx), "baixando mpv...".into())
            }
        };

        Self {
            cfg,
            cmd_tx,
            ev_rx,
            playlist,
            idx: None,
            now: String::new(),
            time: 0.0,
            duration: 0.0,
            paused: false,
            volume: 100.0,
            resume: resume::load(),
            status: status_msg,
            mpv_check,
            initial_play,
            embed: None,
        }
    }

    fn play(&mut self, i: usize) {
        let Some(path) = self.playlist.files.get(i).cloned() else {
            return;
        };
        let r = resume::position_of(&self.resume, &path);
        let wid = self.embed.as_ref().map(|e| e.child_hwnd());
        let _ = self.cmd_tx.send(Cmd::Open {
            path,
            resume: r,
            volume: self.volume,
            wid,
        });
        self.idx = Some(i);
        self.now = self
            .playlist
            .files
            .get(i)
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.time = 0.0;
        self.duration = 0.0;
        self.paused = false;
        self.status.clear();
    }

    fn next(&mut self) {
        if let Some(i) = self.idx {
            if i + 1 < self.playlist.files.len() {
                self.play(i + 1);
            }
        }
    }

    fn mpv_ready(&self) -> bool {
        self.mpv_check.is_none()
    }
}

fn fmt_time(s: f64) -> String {
    if s.is_nan() || s < 0.0 {
        return "--:--".into();
    }
    let t = s as u64;
    let (h, m, sec) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Checa se o download do mpv terminou.
        if let Some(rx) = &self.mpv_check {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(_path) => {
                        self.status.clear();
                        eprintln!("[lab-player] mpv pronto");
                    }
                    Err(e) => self.status = format!("⚠ mpv: {e}"),
                }
                self.mpv_check = None;
            } else {
                // Ainda baixando — pede repaint e mostra spinner.
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        // Garante o child de vídeo (janela já existe no primeiro update;
        // se o FindWindow ainda não achou, tenta de novo no próximo frame).
        if self.embed.is_none() {
            self.embed = embed::VideoEmbed::new("Lab Player");
        }

        // "Abrir com": toca o arquivo dos args assim que o mpv estiver
        // disponível (imediatamente se já estava pronto, ou quando o
        // download terminar).
        if let Some(i) = self.initial_play {
            if self.mpv_ready() {
                self.play(i);
                self.initial_play = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        // Drag & drop de arquivos (o placeholder promete; aqui entrega).
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|d| d.path.clone().map(|p| p.display().to_string()))
                .collect()
        });
        if !dropped.is_empty() {
            let mut added = false;
            for f in dropped {
                if is_media(std::path::Path::new(&f)) && !self.playlist.files.contains(&f) {
                    self.playlist.files.push(f);
                    added = true;
                }
            }
            if added {
                resume::save_playlist(&self.playlist);
            }
        }

        // Eventos do motor.
        let mut ended = false;
        while let Ok(ev) = self.ev_rx.try_recv() {
            match ev {
                Event::Time(t, d) => {
                    if !t.is_nan() {
                        self.time = t;
                        // Salva o resume a cada ~5 s (barato: 200 entradas).
                        if let (Some(i), true) = (self.idx, (t.trunc() % 5.0) < 0.1) {
                            if let Some(p) = self.playlist.files.get(i) {
                                resume::remember(&mut self.resume, p, t);
                            }
                        }
                    }
                    if !d.is_nan() {
                        self.duration = d;
                    }
                }
                Event::EndFile => ended = true,
                Event::Exited => {
                    self.status = "mpv fechado".into();
                    self.idx = None;
                    self.now.clear();
                }
                Event::Ready => {
                    if self.paused {
                        let _ = self.cmd_tx.send(Cmd::Pause);
                    }
                }
            }
        }
        if ended {
            self.next();
        }

        // Seek de teclado: ←/→ ±10s (passo do mpv), espaço = play/pause.
        if self.idx.is_some() {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                let _ = self.cmd_tx.send(Cmd::SeekRelative(-10.0));
                self.time = (self.time - 10.0).max(0.0);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let _ = self.cmd_tx.send(Cmd::SeekRelative(10.0));
                self.time += 10.0;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.paused = !self.paused;
                let _ = self.cmd_tx.send(if self.paused {
                    Cmd::Pause
                } else {
                    Cmd::Unpause
                });
            }
        }

        // Motor vivo → frames rápidos (seek bar).
        if self.idx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Player");
                ui.label(egui::RichText::new(&self.now).small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if lab_ui::settings_ui(ui, &mut self.cfg) {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("controles").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let playing = self.idx.is_some() && self.mpv_ready();
                let label = if self.paused { "▶" } else { "⏸" };
                if ui
                    .add_enabled(playing, egui::Button::new(label))
                    .clicked()
                {
                    self.paused = !self.paused;
                    let _ = self.cmd_tx.send(if self.paused {
                        Cmd::Pause
                    } else {
                        Cmd::Unpause
                    });
                }
                if ui
                    .add_enabled(playing, egui::Button::new("⏹"))
                    .clicked()
                {
                    let _ = self.cmd_tx.send(Cmd::Stop);
                    self.idx = None;
                    self.now.clear();
                }
                ui.label(format!(
                    "{} / {}",
                    fmt_time(self.time),
                    fmt_time(self.duration)
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.volume, 0.0..=100.0)
                            .text("♪"),
                    )
                    .changed()
                    .then(|| {
                        let _ = self.cmd_tx.send(Cmd::Volume(self.volume));
                    });
                });
            });
            // Seek bar.
            let playing = self.idx.is_some() && self.mpv_ready();
            let mut t = self.time;
            let slider = ui.add_enabled(
                playing,
                egui::Slider::new(&mut t, 0.0..=self.duration.max(1.0))
                    .show_value(false),
            );
            if slider.drag_stopped() && t != self.time {
                let _ = self.cmd_tx.send(Cmd::SeekAbsolute(t));
                self.time = t;
            }
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(&self.status).small().weak());
            }
            if self.mpv_check.is_some() {
                ui.spinner();
            }
        });

        // Playlist: painel inferior fixo entre os controles e o vídeo.
        egui::TopBottomPanel::bottom("playlist")
            .exact_height(140.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let t = i18n::t(self.cfg.lang, Key::Items);
                    ui.strong(t);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("+ arquivo").clicked() {
                            if let Some(p) = pick_media() {
                                self.playlist.files.push(p);
                                resume::save_playlist(&self.playlist);
                            }
                        }
                        if ui.button("+ pasta").clicked() {
                            if let Some(dir) = pick_dir() {
                                if let Ok(files) = list_media(&dir) {
                                    self.playlist.files.extend(files);
                                    resume::save_playlist(&self.playlist);
                                }
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.playlist.files.is_empty(),
                                egui::Button::new(i18n::t(self.cfg.lang, Key::Clear)),
                            )
                            .clicked()
                        {
                            self.playlist.files.clear();
                            self.idx = None;
                            resume::save_playlist(&self.playlist);
                        }
                    });
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, f) in self.playlist.files.clone().iter().enumerate() {
                        let name = std::path::Path::new(f)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| f.clone());
                        let is_now = self.idx == Some(i);
                        if ui
                            .selectable_label(is_now, &name)
                            .clicked()
                            && self.mpv_ready()
                        {
                            self.play(i);
                        }
                    }
                });
            });

        // Vídeo: painel central. O mpv desenha no child window posicionado
        // sobre este retângulo (DWM compõe acima da superfície GL).
        let ppp = ctx.pixels_per_point();
        let mut video_rect = egui::Rect::NOTHING;
        egui::CentralPanel::default().show(ctx, |ui| {
            video_rect = ui.max_rect();
            if self.idx.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("arraste mídia aqui ou selecione na playlist")
                            .weak(),
                    );
                });
            }
        });

        // Reposiciona o child do vídeo a cada frame (barato; resize/dpi
        // saem de graça) e alterna a visibilidade com o estado do play.
        if let Some(e) = &self.embed {
            let min = video_rect.min * ppp;
            let size = video_rect.size() * ppp;
            e.place(
                min.x.round() as i32,
                min.y.round() as i32,
                size.x.round() as i32,
                size.y.round() as i32,
            );
            e.set_visible(self.idx.is_some() && self.mpv_ready());
        }
    }
}

fn pick_media() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .add_filter(
                "Mídia",
                &["mp4", "mkv", "webm", "mov", "avi", "m4v", "mp3", "flac", "ogg", "wav", "m4a"],
            )
            .pick_file()
            .map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        None // Linux: caminho digitado (política do lab: AppImage enxuto)
    }
}

fn pick_dir() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().pick_folder().map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn list_media(dir: &str) -> Result<Vec<String>, String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| is_media(&e.path()))
        .map(|e| e.path().display().to_string())
        .collect();
    files.sort_by_key(|f| f.to_lowercase());
    Ok(files)
}
