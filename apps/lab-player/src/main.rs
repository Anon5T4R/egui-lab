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
use std::time::{Duration, Instant};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use lab_ui::workarea::fit_to_work_area;
use mpv::{Cmd, Event};

const APP_ID: &str = "lab-player";

/// Extensões aceitas na playlist (o filtro do diálogo, do drag-drop e dos args).
const MEDIA_EXTS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "avi", "m4v", "wmv", "mp3", "flac", "ogg", "wav", "m4a", "opus",
];

fn is_media(p: &std::path::Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .map(|x| MEDIA_EXTS.contains(&x.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn main() -> eframe::Result<()> {
    // Embed via --wid exige X11 dos dois lados (janela do app e mpv).
    // Sem isso o winit prefere Wayland nativo, onde não existe XID. Com a
    // variável removida cai pro XWayland — presente em praticamente
    // qualquer desktop; e o mpv filho herda o env, ficando X11 também.
    #[cfg(target_os = "linux")]
    {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("WAYLAND_SOCKET");
    }

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
    /// Child window que recebe o vídeo (`--wid`). Windows: HWND; Linux: XID.
    embed: Option<embed::VideoEmbed>,
    /// Embed descartado (sessão sem X11) — não tenta mais, mpv em janela
    /// própria.
    embed_dead: bool,
    /// Tentativas de criar o embed: deadline por TEMPO (contar frames
    /// mente — com repaint adaptativo o frame é esparso).
    embed_deadline: Option<Instant>,
    /// Último tamanho de janela aplicado por Dims (dedupe: mpv manda um
    /// property-change POR EIXO — sem isso são dois resizes por vídeo).
    placed_size: Option<[f32; 2]>,
    /// Seek bar em drag (o valor vive aqui — reinicializar do `time` a
    /// cada frame faz o slider brigar com o playback).
    seek_drag: Option<f64>,
    /// Rotação do vídeo em quartos de volta horário (0..3) — o mpv gira
    /// via `video-rotate` (runtime, reaplicado a cada arquivo).
    rot: u32,
    /// Listagem de "+ pasta" rodando fora da UI thread (read_dir em pasta
    /// de rede/nuvem trava a janela — mesma lentidão do Explorer).
    dir_rx: Option<Receiver<Result<Vec<String>, String>>>,
    /// Resume com mudanças ainda não persistidas.
    resume_dirty: bool,
    /// Interface visível (controles/playlist). Escondida enquanto o vídeo
    /// roda — clique ou F traz de volta.
    chrome: bool,
    /// Dimensões do vídeo atual (px) — redimensiona a janela pro tamanho
    /// do vídeo quando a interface está escondida.
    video_size: Option<[f32; 2]>,
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
        let initial_play = first_arg.and_then(|f| playlist.files.iter().position(|x| *x == f));
        if initial_play.is_some() {
            resume::save_playlist(&playlist);
        }

        // Checa se o mpv está disponível. Se não, dispara download em
        // background (Windows) ou registra o erro (Linux sem mpv no PATH).
        let (mpv_check, status_msg) = match mpv_setup::check() {
            mpv_setup::MpvStatus::Ready => (None, String::new()),
            mpv_setup::MpvStatus::NeedsDownload(msg) => {
                // Só existe no Windows — no Linux o check() nunca retorna
                // NeedsDownload (o download é build Windows).
                #[cfg(windows)]
                {
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(mpv_setup::download());
                    });
                    eprintln!("[lab-player] {msg}");
                    (Some(rx), "baixando mpv...".into())
                }
                #[cfg(not(windows))]
                {
                    let _ = msg;
                    (None, String::new())
                }
            }
            mpv_setup::MpvStatus::Error(e) => (None, format!("⚠ {e}")),
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
            embed_dead: false,
            embed_deadline: Some(Instant::now() + Duration::from_secs(3)),
            placed_size: None,
            seek_drag: None,
            rot: 0,
            dir_rx: None,
            resume_dirty: false,
            chrome: true,
            video_size: None,
        }
    }

    fn play(&mut self, i: usize) {
        let Some(path) = self.playlist.files.get(i).cloned() else {
            return;
        };
        // Persiste o progresso da faixa anterior antes de trocar (o
        // checkpoint periódico é só em memória — ver Event::Time).
        self.flush_resume();
        let r = resume::position_of(&self.resume, &path);
        let wid = self.embed.as_ref().map(|e| e.child_handle());
        let _ = self.cmd_tx.send(Cmd::Open {
            path,
            resume: r,
            volume: self.volume,
            wid,
        });
        self.idx = Some(i);
        // Tocando = só o vídeo: interface some (clique ou F traz de volta).
        self.chrome = false;
        self.video_size = None;
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

    /// Escreve o resume.json em disco (fora do caminho quente da UI —
    /// chamar em troca de faixa/fim/exit; o tick de 5 s é só memória).
    fn flush_resume(&mut self) {
        if self.resume_dirty {
            resume::save(&self.resume);
            self.resume_dirty = false;
        }
    }

    /// Janela do tamanho do vídeo (só com interface oculta — com painéis
    /// o resize seria redundante). Dedupe pelo tamanho JÁ GIRADO: só
    /// refaz quando os dois eixos chegaram e mudaram (mpv re-envia
    /// width/height a cada arquivo; girar 90° troca os eixos e conta
    /// como mudança).
    fn fit_window_to_video(&mut self, ctx: &egui::Context) {
        let Some([vw, vh]) = self.video_size else { return };
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        // mpv reporta as dims da FONTE — rotação de 90°/270° troca os
        // eixos do que é exibido.
        let (dw, dh) = if self.rot % 2 == 1 {
            (vh, vw)
        } else {
            (vw, vh)
        };
        if self.placed_size == Some([dw, dh]) || self.chrome || self.embed_dead {
            return;
        }
        if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            return;
        }
        self.placed_size = Some([dw, dh]);
        let (inner, pos) = fit_to_work_area(ctx, dw, dh);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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

        // Garante o child de vídeo (Windows: HWND por título; Linux: XID
        // via raw-window-handle/X11). None pode ser "ainda não" (janela
        // não nasceu) ou "impossível" (Wayland puro) — tenta por uns 3 s
        // (tempo, não frames: com repaint adaptativo o frame é esparso)
        // e desiste.
        if self.embed.is_none() && !self.embed_dead {
            if let Some(e) = embed::VideoEmbed::new("Lab Player", frame) {
                self.embed = Some(e);
                self.embed_deadline = None;
            } else if self
                .embed_deadline
                .map(|d| Instant::now() >= d)
                .unwrap_or(false)
            {
                self.embed_dead = true;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        // Listagem de "+ pasta" chegou (rodou fora da UI thread).
        if let Some(rx) = &self.dir_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(files) => {
                        self.playlist.files.extend(files);
                        resume::save_playlist(&self.playlist);
                    }
                    Err(e) => self.status = format!("⚠ {e}"),
                }
                self.dir_rx = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
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
                        // Checkpoint em MEMÓRIA a cada ~5 s (o disco é
                        // tocado só em troca de faixa/fim/saída — write
                        // na UI thread a cada 5 s era stutter).
                        if let (Some(i), true) = (self.idx, (t.trunc() % 5.0) < 0.1) {
                            if let Some(p) = self.playlist.files.get(i) {
                                resume::remember(&mut self.resume, p, t);
                                self.resume_dirty = true;
                            }
                        }
                    }
                    if !d.is_nan() {
                        self.duration = d;
                    }
                }
                Event::Dims(w, h) => {
                    // Acumula eixos (mpv manda um property-change por eixo).
                    let size = self.video_size.get_or_insert([0.0, 0.0]);
                    if w > 0 {
                        size[0] = w as f32;
                    }
                    if h > 0 {
                        size[1] = h as f32;
                    }
                    self.fit_window_to_video(ctx);
                }
                Event::EndFile => ended = true,
                Event::Exited => {
                    self.status = "mpv fechado".into();
                    self.idx = None;
                    self.now.clear();
                    self.flush_resume();
                    // Sem vídeo → interface de volta (usuário precisa dos
                    // botões pra escolher outra coisa).
                    self.chrome = true;
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
            // [ / ] = girar o vídeo 90° (anti-horário / horário) — igual
            // ao lab-image. A janela acompanha (90° troca os eixos).
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                self.rot = (self.rot + 3) % 4;
                let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                self.fit_window_to_video(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                self.rot = (self.rot + 1) % 4;
                let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                self.fit_window_to_video(ctx);
            }
        }

        // F = tela cheia sem interface (o modelo "viewer"); Esc volta.
        if ctx.input(|i| i.key_pressed(egui::Key::F)) {
            let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }

        // Clique na área do vídeo (capturado pelo child — WndProc no
        // Windows, ButtonPress no X11) alterna a interface. Com o child
        // oculto (parado), o clique chega no egui normalmente.
        if let Some(e) = &self.embed {
            if e.take_click() {
                self.chrome = !self.chrome;
                // Interface escondida = modo viewer: a janela assume o
                // tamanho do vídeo (os Dims já vieram, mas com painéis
                // abertos o resize é pulado — agora é a hora).
                if !self.chrome {
                    self.fit_window_to_video(ctx);
                }
            }
        }

        // Repaint adaptativo: os eventos do motor chegam por canal (só
        // são drenados num frame), então tocando precisa de ticks — mas
        // interface oculta não tem seek bar pra animar, e pausado não
        // tem progresso: 120 ms vira 400 ms nos dois casos.
        if self.idx.is_some() {
            let ms = if self.chrome && !self.paused {
                120
            } else {
                400
            };
            ctx.request_repaint_after(Duration::from_millis(ms));
        }

        // Interface (topo/controles/playlist) só quando `chrome` — tocando,
        // a janela é só o vídeo.
        let chrome = self.chrome;
        if chrome {
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
        }

        if chrome {
            egui::TopBottomPanel::bottom("controles").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let playing = self.idx.is_some() && self.mpv_ready();
                    let label = if self.paused { "▶" } else { "⏸" };
                    if ui.add_enabled(playing, egui::Button::new(label)).clicked() {
                        self.paused = !self.paused;
                        let _ = self.cmd_tx.send(if self.paused {
                            Cmd::Pause
                        } else {
                            Cmd::Unpause
                        });
                    }
                    if ui.add_enabled(playing, egui::Button::new("⏹")).clicked() {
                        let _ = self.cmd_tx.send(Cmd::Stop);
                        self.idx = None;
                        self.now.clear();
                    }
                    if ui
                        .add_enabled(playing, egui::Button::new("⟳"))
                        .on_hover_text("girar 90° ([ e ] também giram)")
                        .clicked()
                    {
                        self.rot = (self.rot + 1) % 4;
                        let _ = self.cmd_tx.send(Cmd::Rotate((self.rot * 90) as i32));
                        self.fit_window_to_video(ctx);
                    }
                    ui.label(format!(
                        "{} / {}",
                        fmt_time(self.time),
                        fmt_time(self.duration)
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Slider::new(&mut self.volume, 0.0..=100.0).text("♪"))
                            .changed()
                            .then(|| {
                                let _ = self.cmd_tx.send(Cmd::Volume(self.volume));
                            });
                    });
                });
                // Seek bar. Durante o drag o valor vive em `seek_drag` —
                // reinicializar do `time` a cada frame fazia o slider
                // brigar com os eventos de progresso do mpv.
                let playing = self.idx.is_some() && self.mpv_ready();
                let mut t = self.seek_drag.unwrap_or(self.time);
                let slider = ui.add_enabled(
                    playing,
                    egui::Slider::new(&mut t, 0.0..=self.duration.max(1.0)).show_value(false),
                );
                if slider.dragged() {
                    self.seek_drag = Some(t);
                }
                if slider.drag_stopped() {
                    self.seek_drag = None;
                    if t != self.time {
                        let _ = self.cmd_tx.send(Cmd::SeekAbsolute(t));
                        self.time = t;
                    }
                }
                if !self.status.is_empty() {
                    ui.label(egui::RichText::new(&self.status).small().weak());
                }
                if self.mpv_check.is_some() {
                    ui.spinner();
                }
            });
        }

        // Playlist: painel inferior fixo entre os controles e o vídeo.
        if chrome {
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
                                    // Fora da UI thread: read_dir em pasta
                                    // de rede/nuvem (OneDrive) tem a MESMA
                                    // lentidão do Explorer — na UI thread
                                    // travava a janela inteira.
                                    let (tx, rx) = std::sync::mpsc::channel();
                                    std::thread::spawn(move || {
                                        let _ = tx.send(list_media(&dir));
                                    });
                                    self.dir_rx = Some(rx);
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
                        // Sem clone por frame: só os índices (a playlist
                        // inteira era clonada a cada repaint).
                        for i in 0..self.playlist.files.len() {
                            let name = std::path::Path::new(&self.playlist.files[i])
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| self.playlist.files[i].clone());
                            let is_now = self.idx == Some(i);
                            if ui.selectable_label(is_now, &name).clicked() && self.mpv_ready() {
                                self.play(i);
                            }
                        }
                    });
                });
        }

        // Vídeo: painel central. O mpv desenha no child window posicionado
        // sobre este retângulo (DWM compõe acima da superfície GL).
        let ppp = ctx.pixels_per_point();
        let mut video_rect = egui::Rect::NOTHING;
        let mut clicked_idle = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            video_rect = ui.max_rect();
            if self.idx.is_none() {
                // Parado (child oculto): o clique chega no egui — mostra a
                // interface de volta.
                let resp = ui
                    .centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("arraste mídia aqui ou selecione na playlist")
                                .weak(),
                        );
                    })
                    .response;
                clicked_idle = resp.clicked();
            }
        });
        if clicked_idle {
            self.chrome = true;
        }

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

    /// Fechou o app no meio do vídeo: persiste o último checkpoint.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_resume();
    }
}

fn pick_media() -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .add_filter(
                "Mídia",
                &[
                    "mp4", "mkv", "webm", "mov", "avi", "m4v", "mp3", "flac", "ogg", "wav", "m4a",
                ],
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
        rfd::FileDialog::new()
            .pick_folder()
            .map(|p| p.display().to_string())
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
