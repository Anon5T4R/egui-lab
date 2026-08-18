//! lab-clip — piloto de referência do LocalClip em egui/eframe.
//! O teste desta onda é INTEGRAÇÃO OS sem Tauri: bandeja (`tray-icon`),
//! atalho global (`global-hotkey`, ambos crates que o próprio Tauri usa por
//! baixo) e poller de clipboard (`arboard` + flag de privacidade do Windows).
//!
//! Diferenças deliberadas do oficial: só texto (sem imagem), histórico em
//! memória (sem SQLite) e hotkey **Ctrl+Alt+V** em vez de Ctrl+Shift+V — o
//! LocalClip instalado é dono do Ctrl+Shift+V; dois apps não registram o
//! mesmo atalho. Fechar no X encerra de verdade (o "fechar pra bandeja" do
//! oficial é opt-in lá, e aqui não configuramos).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod poller;

use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;
use tray_icon::menu::{Menu, MenuItem, MenuEvent};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use history::ClipItem;

const APP_ID: &str = "lab-clip";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Clip")
            .with_inner_size([420.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, cfg.theme);
            Ok(Box::new(ClipApp::new(cfg)))
        }),
    )
}

struct ClipApp {
    cfg: Config,
    items: Vec<ClipItem>,
    search: String,
    rx: Receiver<String>,
    /// Viva enquanto o app viver: derruba a bandeja e o atalho no drop.
    _tray: TrayIcon,
    _hotkeys: GlobalHotKeyManager,
    visible: bool,
    focus_search: bool,
}

impl ClipApp {
    fn new(cfg: Config) -> Self {
        let rx = poller::spawn();

        // Bandeja: menu mínimo + toggle no clique esquerdo (paridade do oficial).
        let show = MenuItem::with_id("show", "Mostrar/Ocultar", true, None);
        let quit = MenuItem::with_id("quit", "Sair", true, None);
        let menu = Menu::new();
        let _ = menu.append(&show);
        let _ = menu.append(&quit);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Lab Clip (Ctrl+Alt+V)")
            .with_icon(tray_icon().expect("ícone rgba"))
            .build()
            .expect("tray");

        // Atalho global: Ctrl+Alt+V (o oficial é Ctrl+Shift+V — ver header).
        let hotkeys = GlobalHotKeyManager::new().expect("manager de hotkey");
        hotkeys
            .register(HotKey::new(
                Some(Modifiers::CONTROL | Modifiers::ALT),
                Code::KeyV,
            ))
            .expect("registrar Ctrl+Alt+V");

        Self {
            cfg,
            items: Vec::new(),
            search: String::new(),
            rx,
            _tray: tray,
            _hotkeys: hotkeys,
            visible: true,
            focus_search: false,
        }
    }

    /// Mostra/esconde, como o handler do oficial: visível E focado → esconde;
    /// senão mostra, foca a janela e prepara o foco da busca.
    fn toggle(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if self.visible && focused {
            self.visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else {
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus_search = true;
        }
    }

    fn copy_out(text: &str) {
        // O poller ignora este copy (SKIP_NEXT) — não vira item novo.
        poller::SKIP_NEXT.store(true, Ordering::Relaxed);
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(text.to_string());
        }
    }
}

/// Ícone 32×32 gerado em código (lab não carrega assets): uma "prancheta"
/// rosa com placa clara — nada de texto, 32px é pouco pra letra legível.
fn tray_icon() -> Option<tray_icon::Icon> {
    let (w, h) = (32u32, 32u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let plate = x >= 9 && x < w - 9 && y >= 6 && y < h - 8;
            let clip = x >= 13 && x < w - 13 && y >= 3 && y < 7; // grampo
            let (r, g, b, a) = if clip || plate {
                (247, 237, 242, 255)
            } else if edge {
                (90, 50, 70, 255)
            } else {
                (201, 116, 159, 255)
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

impl eframe::App for ClipApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── eventos de fora (bandeja, atalho, poller) ────────────────────
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            match ev.id.0.as_str() {
                "show" => self.toggle(ctx),
                "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Exit),
                _ => {}
            }
        }
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                self.toggle(ctx);
            }
        }
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state() == HotKeyState::Pressed {
                self.toggle(ctx);
            }
        }
        while let Ok(text) = self.rx.try_recv() {
            history::insert(&mut self.items, text);
        }

        // Poller fala por canal; sem repaint explícito a UI não drenaria.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Clip");
                ui.label(
                    egui::RichText::new("Ctrl+Alt+V").small().weak(),
                );
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
            let t = |k: Key| i18n::t(lang, k);

            let search_id = egui::Id::new("clip-search");
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id(search_id)
                    .hint_text(t(Key::Search))
                    .desired_width(f32::INFINITY),
            );
            if self.focus_search {
                if edit.has_focus() {
                    self.focus_search = false; // já focou (janela estava visível)
                } else {
                    ui.memory_mut(|m| m.request_focus(search_id));
                }
            }

            ui.add_space(6.0);

            let needle = self.search.trim().to_lowercase();
            let view: Vec<usize> = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| needle.is_empty() || i.text.to_lowercase().contains(&needle))
                .map(|(idx, _)| idx)
                .collect();

            if self.items.is_empty() {
                ui.label(egui::RichText::new(t(Key::Empty)).weak());
            } else if view.is_empty() {
                ui.label(egui::RichText::new(t(Key::Empty)).weak());
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for idx in view {
                        // Dados owned ANTES do closure: o closure muta self.items
                        // (pin/delete), então nenhum borrow pode atravessá-lo.
                        let pinned = self.items[idx].pinned;
                        let text_full = self.items[idx].text.clone();
                        let preview = if text_full.len() > 120 {
                            format!("{}…", &text_full[..120])
                        } else {
                            text_full.clone()
                        };
                        ui.horizontal(|ui| {
                            if ui
                                .small_button(if pinned { "📌" } else { "○" })
                                .on_hover_text(if pinned {
                                    t(Key::Unpin)
                                } else {
                                    t(Key::Pin)
                                })
                                .clicked()
                            {
                                self.items[idx].pinned = !self.items[idx].pinned;
                                // Fixado vai pro topo, como favorito.
                                let it = self.items.remove(idx);
                                self.items.insert(0, it);
                            }
                            // Clique no texto = copiar e esconder (fluxo do popup).
                            if ui
                                .selectable_label(false, &preview)
                                .on_hover_text(t(Key::Copy))
                                .clicked()
                            {
                                Self::copy_out(&text_full);
                                self.toggle(ctx);
                            }
                            if ui.small_button("⧉").on_hover_text(t(Key::Copy)).clicked() {
                                Self::copy_out(&text_full);
                            }
                            if ui
                                .small_button("🗑")
                                .on_hover_text(t(Key::Delete))
                                .clicked()
                            {
                                self.items.remove(idx);
                            }
                        });
                    }
                });
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    let soltos = self.items.iter().filter(|i| !i.pinned).count();
                    ui.label(
                        egui::RichText::new(format!("{}: {}", t(Key::Items), soltos))
                            .small()
                            .weak(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(!soltos.is_empty(), egui::Button::new(t(Key::Clear)))
                            .clicked()
                        {
                            history::clear_unpinned(&mut self.items);
                        }
                    });
                });
            });
        });
    }
}
