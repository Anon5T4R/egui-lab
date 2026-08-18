//! lab-calc — piloto de referência do LocalCalc em egui/eframe.
//! Escopo da onda 1: modo padrão (expressão livre + preview ao vivo +
//! histórico em memória + teclado). Sem científica/programador/conversor:
//! o objetivo é validar o port do "padrão" (tema/i18n/config) e a ergonomia
//! de forms/teclado no egui, não paridade com o app oficial.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;

use eframe::egui;
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;

const APP_ID: &str = "lab-calc";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Calc")
            .with_inner_size([400.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(CalcApp::new(cfg)))
        }),
    )
}

struct CalcApp {
    cfg: Config,
    expr: String,
    result: Option<String>,
    history: Vec<(String, String)>,
}

impl CalcApp {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            expr: String::new(),
            result: None,
            history: Vec::new(),
        }
    }

    fn commit(&mut self) {
        let src = self.expr.trim().to_string();
        if src.is_empty() {
            return;
        }
        match engine::eval(&src) {
            Ok(v) => {
                let out = engine::fmt_num(v);
                self.result = Some(out.clone());
                self.history.insert(0, (src, out));
            }
            Err(e) => {
                self.result = Some(format!("{}: {e}", i18n::t(self.cfg.lang, Key::Error)));
            }
        }
        self.expr.clear();
    }
}

impl eframe::App for CalcApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Calc");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if lab_ui::settings_ui(ui, &mut self.cfg) {
                        theme::apply(ctx, self.cfg.theme);
                        let _ = config::save(APP_ID, &self.cfg);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let lang = self.cfg.lang;

            // Resultado (ou erro) da última conta, grande, à direita; sem conta,
            // mostra o preview ao vivo do que está digitado.
            let shown = self.result.clone().unwrap_or_else(|| {
                self.preview()
                    .map(|v| format!("= {}", engine::fmt_num(v)))
                    .unwrap_or_default()
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.set_min_height(38.0);
                ui.label(egui::RichText::new(shown).size(26.0).strong());
            });

            ui.add_space(4.0);

            // Campo de expressão: Enter confirma (preview ao vivo enquanto digita).
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.expr)
                    .hint_text(i18n::t(lang, Key::ExprHint))
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.commit();
                ui.memory_mut(|m| m.request_focus(edit.id));
            }

            ui.add_space(6.0);

            // Teclado: 5 colunas; ÷ × − são display, entra o operador real.
            egui::Grid::new("pad")
                .num_columns(5)
                .min_col_width(44.0)
                .spacing([6.0, 6.0])
                .show(ui, |ui| {
                    let rows: [[&str; 5]; 4] = [
                        ["7", "8", "9", "/", "C"],
                        ["4", "5", "6", "*", "DEL"],
                        ["1", "2", "3", "-", "^"],
                        ["0", ".", "(", ")", "+"],
                    ];
                    for row in rows {
                        for label in row {
                            if ui.button(label).clicked() {
                                self.key(label);
                            }
                        }
                        ui.end_row();
                    }
                });

            ui.add_space(6.0);
            if ui
                .add_sized([ui.available_width(), 34.0], egui::Button::new("="))
                .clicked()
            {
                self.commit();
            }

            ui.add_space(10.0);
            ui.label(egui::RichText::new(i18n::t(lang, Key::History)).small().weak());

            if self.history.is_empty() {
                ui.label(egui::RichText::new(i18n::t(lang, Key::NoHistory)).weak());
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (src, out) in &self.history {
                        let label = format!("{src} = {out}");
                        if ui
                            .selectable_label(false, label.as_str())
                            .on_hover_text("clique reusa")
                            .clicked()
                        {
                            self.expr = src.clone();
                        }
                    }
                });
            }
        });
    }
}

impl CalcApp {
    fn preview(&self) -> Option<f64> {
        if self.expr.trim().is_empty() {
            return None;
        }
        engine::eval(&self.expr).ok()
    }

    fn key(&mut self, label: &str) {
        match label {
            "C" => {
                self.expr.clear();
                self.result = None;
            }
            "DEL" => {
                self.expr.pop();
            }
            "/" => self.expr.push('/'),
            "*" => self.expr.push('*'),
            "-" => self.expr.push('-'),
            "+" => self.expr.push('+'),
            other => self.expr.push_str(other),
        }
    }
}
