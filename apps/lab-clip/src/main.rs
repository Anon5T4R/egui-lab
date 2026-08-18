//! lab-clip — piloto de referência do LocalClip em egui/eframe.
//! Onda 2: integração OS sem Tauri (bandeja no Windows, atalho global,
//! poller com a flag de privacidade). Onda 4: IMAGENS — captura → PNG na
//! thread do poller → textura egui (cache por item, liberada ao excluir) →
//! recopiar de volta pro clipboard. O gerenciamento de textura é parte do
//! teste: no immediate mode, quem cria o pixel também tem que apagar.
//!
//! Diferenças deliberadas do oficial: histórico em memória (sem SQLite),
//! hotkey **Ctrl+Alt+V** (o LocalClip instalado é dono do Ctrl+Shift+V) e
//! bandeja só no Windows (tray-icon puxaria gtk3 no Linux — ver Cargo.toml).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod poller;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

use eframe::egui;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use lab_ui::config::{self, Config};
use lab_ui::i18n::{self, Key};
use lab_ui::theme;

#[cfg(windows)]
use tray_icon::menu::{Menu, MenuItem, MenuEvent};
#[cfg(windows)]
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use history::{ClipItem, ImageItem, Payload};

const APP_ID: &str = "lab-clip";

fn main() -> eframe::Result<()> {
    let cfg = config::load(APP_ID);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lab Clip")
            .with_inner_size([440.0, 600.0]),
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
    rx: Receiver<ClipItem>,
    /// Textura de miniatura por id do item — criada sob demanda, REMOVIDA ao
    /// excluir/limpar (textura órfã é leak de VRAM: o custo de aprender isso
    /// em immediate mode é o motivo de estar escrito aqui).
    textures: HashMap<u64, egui::TextureHandle>,
    /// Viva enquanto o app viver: derruba a bandeja e o atalho no drop.
    #[cfg(windows)]
    _tray: TrayIcon,
    _hotkeys: GlobalHotKeyManager,
    visible: bool,
    focus_search: bool,
}

impl ClipApp {
    fn new(cfg: Config) -> Self {
        let rx = poller::spawn();

        // Bandeja: menu mínimo + toggle no clique esquerdo (paridade do
        // oficial). Só no Windows — ver nota no Cargo.toml.
        #[cfg(windows)]
        let tray = {
            let show = MenuItem::with_id("show", "Mostrar/Ocultar", true, None);
            let quit = MenuItem::with_id("quit", "Sair", true, None);
            let menu = Menu::new();
            let _ = menu.append(&show);
            let _ = menu.append(&quit);
            TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Lab Clip (Ctrl+Alt+V)")
                .with_icon(tray_icon().expect("ícone rgba"))
                .build()
                .expect("tray")
        };

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
            textures: HashMap::new(),
            #[cfg(windows)]
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

    fn copy_out_text(text: &str) {
        poller::SKIP_NEXT.store(true, Ordering::Relaxed);
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(text.to_string());
        }
    }

    fn copy_out_image(img: &ImageItem) {
        poller::SKIP_NEXT.store(true, Ordering::Relaxed);
        let Ok(dec) = image::load_from_memory(&img.png) else {
            return;
        };
        let rgba = dec.to_rgba8();
        let data = arboard::ImageData {
            width: rgba.width() as usize,
            height: rgba.height() as usize,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        };
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_image(data);
        }
    }
}

/// Ícone 32×32 gerado em código (lab não carrega assets): uma "prancheta"
/// rosa com placa clara — nada de texto, 32px é pouco pra letra legível.
#[cfg(windows)]
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

/// Linha pronta pra renderizar — dados OWNED (o loop de UI muta self).
enum RowView {
    Text {
        idx: usize,
        id: u64,
        pinned: bool,
        preview: String,
        full: String,
    },
    Image {
        idx: usize,
        id: u64,
        pinned: bool,
        img: ImageItem,
    },
}

impl eframe::App for ClipApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── eventos de fora (bandeja, atalho, poller) ────────────────────
        #[cfg(windows)]
        {
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                match ev.id.0.as_str() {
                    "show" => self.toggle(ctx),
                    "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
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
        }
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state() == HotKeyState::Pressed {
                self.toggle(ctx);
            }
        }
        while let Ok(item) = self.rx.try_recv() {
            history::insert(&mut self.items, item.payload);
        }

        // Poller fala por canal; sem repaint explícito a UI não drenaria.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        egui::TopBottomPanel::top("topo").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Lab Clip");
                ui.label(egui::RichText::new("Ctrl+Alt+V").small().weak());
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

            // Busca filtra TEXTO; imagem não tem texto pra bater — só aparece
            // com busca vazia (mesma conta do oficial).
            let needle = self.search.trim().to_lowercase();
            let view: Vec<RowView> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(idx, it)| match &it.payload {
                    Payload::Text(full) => {
                        if !needle.is_empty() && !full.to_lowercase().contains(&needle) {
                            return None;
                        }
                        let preview = if full.len() > 120 {
                            format!("{}…", &full[..120])
                        } else {
                            full.clone()
                        };
                        Some(RowView::Text {
                            idx,
                            id: it.id,
                            pinned: it.pinned,
                            preview,
                            full: full.clone(),
                        })
                    }
                    Payload::Image(img) => {
                        if !needle.is_empty() {
                            return None;
                        }
                        Some(RowView::Image {
                            idx,
                            id: it.id,
                            pinned: it.pinned,
                            img: img.clone(),
                        })
                    }
                })
                .collect();

            if view.is_empty() {
                ui.label(egui::RichText::new(t(Key::Empty)).weak());
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in &view {
                        match row {
                            RowView::Text {
                                idx,
                                id: _,
                                pinned,
                                preview,
                                full,
                            } => {
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button(if *pinned { "📌" } else { "○" })
                                        .on_hover_text(if *pinned {
                                            t(Key::Unpin)
                                        } else {
                                            t(Key::Pin)
                                        })
                                        .clicked()
                                    {
                                        let i = *idx;
                                        self.items[i].pinned = !self.items[i].pinned;
                                        // Fixado vai pro topo, como favorito.
                                        let it = self.items.remove(i);
                                        self.items.insert(0, it);
                                    }
                                    // Clique no texto = copiar e esconder (fluxo do popup).
                                    if ui
                                        .selectable_label(false, preview.as_str())
                                        .on_hover_text(t(Key::Copy))
                                        .clicked()
                                    {
                                        Self::copy_out_text(full);
                                        self.toggle(ctx);
                                    }
                                    if ui.small_button("⧉").on_hover_text(t(Key::Copy)).clicked() {
                                        Self::copy_out_text(full);
                                    }
                                    if ui
                                        .small_button("🗑")
                                        .on_hover_text(t(Key::Delete))
                                        .clicked()
                                    {
                                        let i = *idx;
                                        let id = self.items[i].id;
                                        self.items.remove(i);
                                        self.textures.remove(&id); // textura órfã = leak de VRAM
                                    }
                                });
                            }
                            RowView::Image { idx, id, pinned, img } => {
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button(if *pinned { "📌" } else { "○" })
                                        .on_hover_text(if *pinned {
                                            t(Key::Unpin)
                                        } else {
                                            t(Key::Pin)
                                        })
                                        .clicked()
                                    {
                                        let i = *idx;
                                        self.items[i].pinned = !self.items[i].pinned;
                                        let it = self.items.remove(i);
                                        self.items.insert(0, it);
                                    }
                                    // Miniatura: textura criada UMA vez por item.
                                    if !self.textures.contains_key(id) {
                                        if let Ok(dec) = image::load_from_memory(&img.png) {
                                            let rgba = dec.to_rgba8();
                                            let size = [rgba.width() as usize, rgba.height() as usize];
                                            let color = egui::ColorImage::from_rgba_unmultiplied(
                                                size, rgba.as_raw(),
                                            );
                                            self.textures.insert(
                                                *id,
                                                ctx.load_texture(
                                                    format!("clip-{id}"),
                                                    color,
                                                    egui::TextureOptions::default(),
                                                ),
                                            );
                                        }
                                    }
                                    if let Some(tex) = self.textures.get(id) {
                                        // Clique na miniatura = copiar e esconder.
                                        if ui
                                            .add(egui::Image::from_texture(tex).max_width(120.0))
                                            .on_hover_text(t(Key::Copy))
                                            .clicked()
                                        {
                                            Self::copy_out_image(img);
                                            self.toggle(ctx);
                                        }
                                    }
                                    if ui.small_button("⧉").on_hover_text(t(Key::Copy)).clicked() {
                                        Self::copy_out_image(img);
                                    }
                                    if ui
                                        .small_button("🗑")
                                        .on_hover_text(t(Key::Delete))
                                        .clicked()
                                    {
                                        let i = *idx;
                                        let removed = self.items.remove(i);
                                        self.textures.remove(&removed.id);
                                    }
                                });
                            }
                        }
                    }
                });
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    let textos = self
                        .items
                        .iter()
                        .filter(|i| matches!(i.payload, Payload::Text(_)) && !i.pinned)
                        .count();
                    let imagens = self
                        .items
                        .iter()
                        .filter(|i| matches!(i.payload, Payload::Image(_)) && !i.pinned)
                        .count();
                    ui.label(
                        egui::RichText::new(format!("{}: {} · 🖼 {}", t(Key::Items), textos, imagens))
                            .small()
                            .weak(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(textos + imagens > 0, egui::Button::new(t(Key::Clear)))
                            .clicked()
                        {
                            // Textura dos que saem também tem que sair.
                            let pinned: Vec<u64> = self
                                .items
                                .iter()
                                .filter(|i| i.pinned)
                                .map(|i| i.id)
                                .collect();
                            self.textures.retain(|id, _| pinned.contains(id));
                            history::clear_unpinned(&mut self.items);
                        }
                    });
                });
            });
        });
    }
}
