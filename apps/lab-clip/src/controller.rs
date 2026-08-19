//! Controller: o "cérebro de bandeja" do lab-clip, numa thread PRÓPRIA.
//!
//! Por que não no `update()`: com a janela oculta o eframe congela (sem
//! WM_PAINT não há frame — ver winctl.rs), então hotkey, bandeja e poller
//! fariam parte do congelamento. Aqui eles vivem fora do ciclo de pintura;
//! o `update()` só lê o estado compartilhado quando a janela está aberta.
//!
//! Comandos chegam de três fontes: atalho global (receiver estático), bandeja
//! (handlers do tray-icon) e segunda instância (arquivo show.flag). A janela
//! é mostrada/escondida via winctl (SO direto), nunca via viewport command —
//! comando de viewport só é processado durante um frame, e frame é exatamente
//! o que não existe com janela oculta.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};

use crate::history::{ClipItem, Payload};

/// Comando vindo da bandeja/atalho.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    ShowHide,
    Quit,
}

/// Estado compartilhado controller ⇄ UI.
pub struct Shared {
    /// Itens novos do poller, esperando a UI adotar (drenado no update).
    pub new_items: Vec<Payload>,
    /// true quando o controller mostrou a janela e a UI deve focar a busca.
    pub want_focus: bool,
    /// Fechar de verdade pedido (bandeja → Sair). A UI, se estiver viva,
    /// segue o caminho limpo (ViewportCommand::Close); o controller já
    /// resolveu o caso janela-oculta via WM_QUIT.
    pub quit: bool,
}

impl Shared {
    fn new() -> Self {
        Self {
            new_items: Vec::new(),
            want_focus: false,
            quit: false,
        }
    }
}

static KEEP_RUNNING: AtomicBool = AtomicBool::new(true);

/// Sobe o controller. Devolve a ponta da UI (canal da bandeja + estado).
pub fn spawn(
    ctx: eframe::egui::Context,
    poller_rx: Receiver<ClipItem>,
) -> (Sender<TrayCmd>, std::sync::Arc<std::sync::Mutex<Shared>>) {
    let (tx, cmd_rx) = std::sync::mpsc::channel::<TrayCmd>();
    let shared = std::sync::Arc::new(std::sync::Mutex::new(Shared::new()));
    let sh = shared.clone();
    let show_flag = lab_ui::config::config_dir(crate::APP_ID).join("show.flag");

    std::thread::Builder::new()
        .name("clip-controller".into())
        .spawn(move || {
            loop {
                if !KEEP_RUNNING.load(Ordering::Relaxed) {
                    break;
                }

                // 1) atalho global (esta thread é a DONA do receiver agora —
                //    o update não drena mais).
                let mut hotkey = false;
                while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
                    if ev.state() == HotKeyState::Pressed {
                        hotkey = true;
                    }
                }

                // 2) bandeja (handlers do tray-icon/muda mandam por canal).
                let mut cmd: Option<TrayCmd> = None;
                while let Ok(c) = cmd_rx.try_recv() {
                    cmd = Some(c);
                }

                // 3) segunda instância pedindo passagem.
                let show_req = show_flag.exists();
                if show_req {
                    let _ = std::fs::remove_file(&show_flag);
                }

                // 4) poller do clipboard alimenta o buffer compartilhado —
                //    captura continua mesmo com a UI congelada/escondida. A
                //    adoção (dedup/pin/teto) acontece na UI; aqui só um teto
                //    de sanidade se a janela nunca abrir.
                let mut had_new = false;
                if let Ok(mut s) = sh.lock() {
                    while let Ok(item) = poller_rx.try_recv() {
                        s.new_items.push(item.payload);
                        had_new = true;
                    }
                    if s.new_items.len() > 1000 {
                        let excess = s.new_items.len() - 1000;
                        s.new_items.drain(..excess);
                    }
                }

                // 5) aplica comandos no SO (nada de viewport command).
                if hotkey {
                    #[cfg(windows)]
                    if crate::winctl::is_visible() {
                        crate::winctl::hide();
                    } else {
                        crate::winctl::show();
                        if let Ok(mut s) = sh.lock() {
                            s.want_focus = true;
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        // Sem bandeja no Linux: o atalho só mostra (esconder
                        // sem bandeja é zumbitar — lá o X fecha de verdade).
                        let _ = hotkey;
                    }
                }
                match cmd {
                    Some(TrayCmd::ShowHide) => {
                        #[cfg(windows)]
                        if crate::winctl::is_visible() {
                            crate::winctl::hide();
                        } else {
                            crate::winctl::show();
                            if let Ok(mut s) = sh.lock() {
                                s.want_focus = true;
                            }
                        }
                    }
                    Some(TrayCmd::Quit) => {
                        #[cfg(windows)]
                        {
                            if crate::winctl::is_visible() {
                                if let Ok(mut s) = sh.lock() {
                                    s.quit = true;
                                }
                                ctx.request_repaint(); // update segue o caminho limpo
                            } else {
                                crate::winctl::force_quit();
                            }
                        }
                        #[cfg(not(windows))]
                        {
                            if let Ok(mut s) = sh.lock() {
                                s.quit = true;
                            }
                            ctx.request_repaint();
                        }
                    }
                    None => {}
                }
                if show_req {
                    #[cfg(windows)]
                    crate::winctl::show();
                    #[cfg(not(windows))]
                    ctx.request_repaint();
                    if let Ok(mut s) = sh.lock() {
                        s.want_focus = true;
                    }
                }

                // Mostrou algo / chegou item novo? Pede frame pra UI
                // sincronizar (visível, o repaint funciona normalmente).
                #[cfg(windows)]
                if (hotkey || cmd.is_some() || show_req || had_new) && crate::winctl::is_visible()
                {
                    ctx.request_repaint();
                }

                std::thread::sleep(Duration::from_millis(120));
            }
        })
        .expect("spawn clip-controller");

    (tx, shared)
}

/// (O controller morre junto com o processo — thread não-main é encerrada
/// pelo runtime quando `main` retorna; não precisa de stop explícito.)
#[allow(dead_code)]
pub fn stop() {
    KEEP_RUNNING.store(false, Ordering::Relaxed);
}
