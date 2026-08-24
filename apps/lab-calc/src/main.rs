//! lab-calc — piloto de referência do LocalCalc em egui/eframe.
//! Onda 1: modo padrão (expressão livre + preview ao vivo + histórico).
//! Onda 4: científica — sin/cos/tan/√/ln/π/e, `ans` e DEG/RAD (mesmo motor
//! do oficial: tokenizer + shunting-yard, agora com funções).

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
            .with_inner_size([400.0, 700.0]),
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
    ans: Option<f64>,
    degrees: bool,
}

impl CalcApp {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            expr: String::new(),
            result: None,
            history: Vec::new(),
            ans: None,
            degrees: false,
        }
    }

    fn ctx(&self) -> engine::Ctx {
        engine::Ctx {
            ans: self.ans,
            degrees: self.degrees,
        }
    }

    fn commit(&mut self) {
        let src = self.expr.trim().to_string();
        if src.is_empty() {
            return;
        }
        match engine::eval_with(&src, &self.ctx()) {
            Ok(v) => {
                // ans vira a última resposta CONFIRMADA (preview não conta).
                self.ans = Some(v);
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
            //
            // GOTCHA do lab (custou a janela vazia da v0.1.0): `with_layout`
            // solto num painel vertical USA TODO O ESPAÇO DISPONÍVEL como
            // min_rect (doc do egui: "If you don't want to use up all available
            // space, use allocate_ui_with_layout") — tudo que vinha depois era
            // empurrado pra fora da janela. Dentro de um `horizontal` ele só
            // come a LINHA, que é o que queremos. Mesma regra dos outros apps.
            let shown = self.result.clone().unwrap_or_else(|| {
                self.preview()
                    .map(|v| format!("= {}", engine::fmt_num(v)))
                    .unwrap_or_default()
            });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.set_min_height(38.0);
                    ui.label(egui::RichText::new(shown).size(26.0).strong());
                });
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

            // Linha de modo: DEG/RAD + indício do ans.
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.degrees, if self.degrees { "DEG" } else { "RAD" })
                    .clicked()
                {
                    self.degrees = !self.degrees;
                }
                if let Some(a) = self.ans {
                    ui.label(
                        egui::RichText::new(format!("ans = {}", engine::fmt_num(a)))
                            .small()
                            .weak(),
                    );
                }
            });

            ui.add_space(4.0);

            // Teclado científico: botão insere o texto (funções já com "(").
            egui::Grid::new("sci")
                .num_columns(5)
                .min_col_width(44.0)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    let rows: [[&str; 5]; 2] = [
                        ["sin(", "cos(", "tan(", "√", "ln("],
                        ["asin(", "acos(", "atan(", "log2(", "abs("],
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

            ui.add_space(4.0);

            // Teclado padrão: 5 colunas.
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

            // π e e entram como texto do motor (mesma coisa que digitar).
            ui.horizontal(|ui| {
                if ui.button("π").clicked() {
                    self.expr.push_str("pi");
                }
                if ui.button("e").clicked() {
                    self.expr.push('e');
                }
                if ui.button("ans").clicked() {
                    self.expr.push_str("ans");
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
            ui.label(
                egui::RichText::new(i18n::t(lang, Key::History))
                    .small()
                    .weak(),
            );

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
        engine::eval_with(&self.expr, &self.ctx()).ok()
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
            "√" => self.expr.push_str("sqrt("),
            "/" => self.expr.push('/'),
            "*" => self.expr.push('*'),
            "-" => self.expr.push('-'),
            "+" => self.expr.push('+'),
            other => self.expr.push_str(other),
        }
    }
}
