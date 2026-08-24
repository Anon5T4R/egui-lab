//! lab-monitor — piloto de referência do LocalMonitor em egui/eframe.
//! Onda 1: CPU total/por núcleo e memória ao vivo, gráficos pintados à mão.
//! Onda 2: tabela de processos (filtro/ordenação/encerrar com confirmação).
//! Onda 4: rede (↓/ú por segundo com histórico) e discos (uso por volume) —
//! paridade de features com o v0.1 do app oficial.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme::{self, Palette};
use sysinfo::{Disks, Networks, Pid, ProcessesToUpdate};

const APP_ID: &str = "lab-monitor";
const MAX_SAMPLES: usize = 160;
const REFRESH: Duration = Duration::from_millis(500);
/// Processos mostrados (pós filtro+ordenação) — centenas de linhas de Grid
/// por frame é o teste de estresse que interessa.
const MAX_ROWS: usize = 200;

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Monitor")
            .with_inner_size([470.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(MonitorApp::new(cfg)))
        }),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortCol {
    Name,
    Pid,
    Cpu,
    Mem,
}

struct MonitorApp {
    cfg: Config,
    sys: sysinfo::System,
    disks: Disks,
    nets: Networks,
    cpu_hist: VecDeque<f32>,
    mem_hist: VecDeque<f32>,
    down_hist: VecDeque<f32>, // bytes/s
    up_hist: VecDeque<f32>,   // bytes/s
    cores: Vec<f32>,
    last_refresh: Instant,
    // processos
    filter: String,
    sort: SortCol,
    sort_desc: bool,
    /// (pid, nome) do processo esperando confirmação de encerramento.
    kill_ask: Option<(Pid, String)>,
}

impl MonitorApp {
    fn new(cfg: Config) -> Self {
        let mut app = Self {
            cfg,
            sys: sysinfo::System::new(),
            disks: Disks::new_with_refreshed_list(),
            nets: Networks::new_with_refreshed_list(),
            cpu_hist: VecDeque::new(),
            mem_hist: VecDeque::new(),
            down_hist: VecDeque::new(),
            up_hist: VecDeque::new(),
            cores: Vec::new(),
            last_refresh: Instant::now(),
            filter: String::new(),
            sort: SortCol::Cpu,
            sort_desc: true,
            kill_ask: None,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let elapsed = self.last_refresh.elapsed().as_secs_f64().max(0.05);
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(true);
        self.nets.refresh(true);

        let cores: Vec<f32> = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let cpu = if cores.is_empty() {
            0.0
        } else {
            cores.iter().sum::<f32>() / cores.len() as f32
        };
        let total = self.sys.total_memory() as f64;
        let mem = if total > 0.0 {
            (self.sys.used_memory() as f64 / total * 100.0) as f32
        } else {
            0.0
        };
        self.cores = cores;
        push_sample(&mut self.cpu_hist, cpu);
        push_sample(&mut self.mem_hist, mem);

        // Rede: `received()` é o delta desde o último refresh — dividir pelo
        // tempo real decorrido dá bytes/s. Soma as interfaces (menos loopback).
        let (rx, tx): (u64, u64) = self
            .nets
            .iter()
            .filter(|(name, _)| !name.starts_with("lo"))
            .map(|(_, d)| (d.received(), d.transmitted()))
            .fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
        push_sample(&mut self.down_hist, (rx as f64 / elapsed) as f32);
        push_sample(&mut self.up_hist, (tx as f64 / elapsed) as f32);
    }

    /// Linha da tabela: dados mínimos clonados do processo (evita segurar o
    /// borrow de `self.sys` enquanto o Grid renderiza).
    fn rows(&self) -> Vec<Row> {
        let needle = self.filter.trim().to_lowercase();
        let mut rows: Vec<Row> = self
            .sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                if needle.is_empty() {
                    return true;
                }
                p.name().to_string_lossy().to_lowercase().contains(&needle)
                    || p.pid().as_u32().to_string().contains(&needle)
            })
            .map(|(pid, p)| Row {
                pid: *pid,
                name: p.name().to_string_lossy().into_owned(),
                cpu: p.cpu_usage(),
                mem_kb: p.memory() / 1024,
            })
            .collect();

        let desc = self.sort_desc;
        match self.sort {
            SortCol::Name => rows.sort_by(|a, b| a.name.cmp(&b.name)),
            SortCol::Pid => rows.sort_by(|a, b| a.pid.cmp(&b.pid)),
            SortCol::Cpu => rows.sort_by(|a, b| a.cpu.total_cmp(&b.cpu)),
            SortCol::Mem => rows.sort_by(|a, b| a.mem_kb.cmp(&b.mem_kb)),
        }
        if desc {
            rows.reverse(); // nome em ordem Z-A quando desc, como no app oficial
        }
        rows.truncate(MAX_ROWS);
        rows
    }

    fn toggle_sort(&mut self, col: SortCol) {
        if self.sort == col {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort = col;
            // Padrão sensato: nome sobe, números descem.
            self.sort_desc = col != SortCol::Name;
        }
    }
}

struct Row {
    pid: Pid,
    name: String,
    cpu: f32,
    mem_kb: u64,
}

fn push_sample(hist: &mut VecDeque<f32>, v: f32) {
    if hist.len() == MAX_SAMPLES {
        hist.pop_front();
    }
    hist.push_back(v);
}

fn fmt_rate(bytes_per_s: f64) -> String {
    if bytes_per_s < 1024.0 {
        format!("{bytes_per_s:.0} B/s")
    } else if bytes_per_s < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_s / 1024.0)
    } else {
        format!("{:.1} MB/s", bytes_per_s / 1024.0 / 1024.0)
    }
}

fn fmt_gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1e9)
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_refresh.elapsed() >= REFRESH {
            self.refresh();
            self.last_refresh = Instant::now();
        }
        // Immediate mode pede repaint explícito quando ocioso — a disciplina
        // que o webview faz sozinho (métrica de interesse do lab).
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Monitor");
                ui.label(fmt_uptime(sysinfo::System::uptime()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if lab_ui::settings_ui(ui, &mut self.cfg) {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let pal = self.cfg.theme.palette();
            let lang = self.cfg.lang;
            let t = |k: Key| i18n::t(lang, k);
            let width = ui.available_width();

            // ── visão geral ─────────────────────────────────────────────
            let cpu_now = self.cpu_hist.back().copied().unwrap_or(0.0);
            chart(
                ui,
                &self.cpu_hist,
                &pal,
                format!("{} · {:.0}%", t(Key::Cpu), cpu_now),
                width,
                90.0,
                true,
            );

            ui.add_space(6.0);

            let mem_now = self.mem_hist.back().copied().unwrap_or(0.0);
            let used_gb = self.sys.used_memory() as f64 / 1e9;
            let total_gb = self.sys.total_memory() as f64 / 1e9;
            chart(
                ui,
                &self.mem_hist,
                &pal,
                format!(
                    "{} · {:.0}% ({:.1}/{:.1} GB)",
                    t(Key::Memory),
                    mem_now,
                    used_gb,
                    total_gb
                ),
                width,
                90.0,
                true,
            );

            ui.add_space(6.0);

            // Rede: ↓ e ↑ lado a lado, escala automática (não é %).
            ui.horizontal(|ui| {
                let half = (width - 6.0) / 2.0;
                let down = self.down_hist.back().copied().unwrap_or(0.0);
                let up = self.up_hist.back().copied().unwrap_or(0.0);
                chart(
                    ui,
                    &self.down_hist,
                    &pal,
                    format!("↓ {}", fmt_rate(down as f64)),
                    half,
                    60.0,
                    false,
                );
                chart(
                    ui,
                    &self.up_hist,
                    &pal,
                    format!("↑ {}", fmt_rate(up as f64)),
                    half,
                    60.0,
                    false,
                );
            });

            ui.add_space(10.0);
            ui.label(egui::RichText::new(t(Key::Cores)).small().weak());

            ui.horizontal_wrapped(|ui| {
                for (i, usage) in self.cores.iter().enumerate() {
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(28.0, 44.0), egui::Sense::hover());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, egui::CornerRadius::same(2), pal.sunken);
                    let h = rect.height() * (usage.clamp(0.0, 100.0) / 100.0);
                    let fill = egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.bottom() - h),
                        egui::vec2(rect.width(), h),
                    );
                    p.rect_filled(fill, egui::CornerRadius::same(2), pal.accent);
                    resp.on_hover_text(format!("{i}: {usage:.0}%"));
                }
            });

            ui.add_space(12.0);
            ui.separator();

            // ── discos ───────────────────────────────────────────────────
            ui.strong(t(Key::Disks));
            ui.add_space(4.0);
            let disk_rows: Vec<(String, u64, u64)> = self
                .disks
                .list()
                .iter()
                .map(|d| {
                    (
                        d.mount_point().to_string_lossy().into_owned(),
                        d.total_space(),
                        d.available_space(),
                    )
                })
                .collect();
            for (mount, total, avail) in disk_rows {
                let used = total.saturating_sub(avail);
                let pct = if total > 0 {
                    used as f32 / total as f32
                } else {
                    0.0
                };
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, egui::CornerRadius::same(2), pal.sunken);
                let fill = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * pct.clamp(0.0, 1.0), rect.height()),
                );
                p.rect_filled(fill, egui::CornerRadius::same(2), pal.accent);
                p.text(
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!(
                        "{mount}  {}/{} ({:.0}%)",
                        fmt_gb(used),
                        fmt_gb(total),
                        pct * 100.0
                    ),
                    egui::FontId::proportional(11.0),
                    pal.text,
                );
                ui.add_space(2.0);
            }

            ui.add_space(10.0);
            ui.separator();

            // ── processos ───────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.strong(t(Key::Processes));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!self.filter.is_empty(), egui::Button::new("✕").small())
                        .on_hover_text(t(Key::Clear))
                        .clicked()
                    {
                        self.filter.clear();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text(t(Key::Search))
                            .desired_width(160.0),
                    );
                });
            });

            ui.add_space(4.0);

            let rows = self.rows();
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("procs")
                    .num_columns(5)
                    .spacing([10.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // cabeçalho clicável (ordenação)
                        let head =
                            |ui: &mut egui::Ui, col: SortCol, label: &str, app: &mut MonitorApp| {
                                let mark = if app.sort == col {
                                    if app.sort_desc {
                                        " ▾"
                                    } else {
                                        " ▴"
                                    }
                                } else {
                                    ""
                                };
                                if ui
                                    .selectable_label(app.sort == col, format!("{label}{mark}"))
                                    .clicked()
                                {
                                    app.toggle_sort(col);
                                }
                            };
                        head(ui, SortCol::Name, t(Key::Name), self);
                        head(ui, SortCol::Pid, "PID", self);
                        head(ui, SortCol::Cpu, "CPU", self);
                        head(ui, SortCol::Mem, "MB", self);
                        ui.label("");
                        ui.end_row();

                        for r in &rows {
                            ui.label(&r.name);
                            ui.label(r.pid.to_string());
                            ui.label(format!("{:.1}%", r.cpu));
                            ui.label(format!("{:.0}", r.mem_kb as f64 / 1024.0));
                            // Encerrar abre confirmação — mesma regra do
                            // app oficial (trabalho não salvo se perde).
                            if ui
                                .add(egui::Button::new(t(Key::End)).small())
                                .on_hover_text(format!("{}: {}", r.pid, r.name))
                                .clicked()
                            {
                                self.kill_ask = Some((r.pid, r.name.clone()));
                            }
                            ui.end_row();
                        }
                    });
            });
        }); // CentralPanel

        // ── diálogo de confirmação de encerramento ─────────────────────
        if let Some((pid, name)) = self.kill_ask.clone() {
            let lang = self.cfg.lang;
            let t = |k: Key| i18n::t(lang, k);
            egui::Window::new(t(Key::KillTitle))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("{} — {}", name, pid));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(t(Key::KillAsk)).weak());
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t(Key::Confirm)).clicked() {
                            // `let _`: compatível com bool (sysinfo ≤0.34) e
                            // Result (0.35+) — o refresh seguinte mostra o
                            // resultado de verdade. (0.33: kill(&self).)
                            if let Some(p) = self.sys.process(pid) {
                                let _ = p.kill();
                            }
                            self.kill_ask = None;
                        }
                        if ui.button(t(Key::Cancel)).clicked() {
                            self.kill_ask = None;
                        }
                    });
                });
        }
    }
}

/// Sparkline pintada à mão — a versão egui das "sparklines próprias em SVG"
/// do LocalMonitor oficial. `percent=true` fixa a escala 0–100 (CPU/mem);
/// `false` auto-escala pelo máximo da série (rede, bytes/s).
fn chart(
    ui: &mut egui::Ui,
    hist: &VecDeque<f32>,
    pal: &Palette,
    caption: String,
    width: f32,
    height: f32,
    percent: bool,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, egui::CornerRadius::same(2), pal.sunken);

    for frac in [0.25f32, 0.5, 0.75] {
        let y = rect.bottom() - rect.height() * frac;
        p.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, pal.grid()),
        );
    }

    let max = if percent {
        100.0
    } else {
        hist.iter().cloned().fold(0.0f32, f32::max).max(1.0)
    };

    let n = hist.len();
    if n >= 2 {
        let px = rect.width() / (MAX_SAMPLES as f32 - 1.0);
        let pts: Vec<egui::Pos2> = hist
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = rect.right() - (n - 1 - i) as f32 * px;
                let y = rect.bottom() - (v.clamp(0.0, max) / max) * (rect.height() - 4.0) - 2.0;
                egui::pos2(x, y)
            })
            .collect();
        // line_segment tem assinatura estável (Into<Stroke>); Painter::line
        // migrou pra PathStroke em 0.31 — segmentos evitam a dependência disso.
        let stroke = egui::Stroke::new(2.0_f32, pal.accent);
        for pair in pts.windows(2) {
            p.line_segment([pair[0], pair[1]], stroke);
        }
    }

    p.text(
        egui::pos2(rect.right() - 8.0, rect.top() + 6.0),
        egui::Align2::RIGHT_TOP,
        caption,
        egui::FontId::proportional(12.0),
        pal.text,
    );
}

fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    format!("{d}d {h}h {m}m")
}
