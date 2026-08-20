//! Motor do lab-player: o mpv roda como PROCESSO SEPARADO (janela própria)
//! e é controlado pelo JSON IPC oficial dele (`--input-ipc-server`).
//! Filosofia idêntica à do LocalPlayer/LocalMedia oficiais: zero build
//! nativo, crash do mpv não derruba o app, código do app MIT.
//!
//! A UI egui é um "controle remoto": play/pause, seek, volume, playlist e
//! resume. Sem embed de vídeo (o oficial embute via `--wid` no Windows e usa
//! janela própria no Linux; o lab usa a janela própria nas duas — o desenho
//! estável documentado do oficial).
//!
//! DESENHO (mais simples que o do oficial, que compartilhava handles com o
//! front Tauri): TODO o mpv vive dentro de UMA thread própria — processo,
//! IPC de escrita e leitura de eventos. O UI só troca mensagens por canais
//! (`Cmd` pra lá, `Event` pra cá). Nenhum handle atravessa a fronteira.
//!
//! Gotcha do Windows (pago pelo oficial em 2026-07-14, preservado aqui):
//! NÃO dá pra ter uma thread leitora bloqueada num ReadFile e escrever no
//! MESMO named pipe por um handle clonado — I/O síncrono no mesmo file
//! object serializa e a escrita fica presa atrás da leitura bloqueada
//! (deadlock: o mpv só manda dados depois que a gente pede, mas pedir é
//! escrever, que está bloqueado). Solução do oficial, usada aqui: POLLING
//! numa thread única — pergunta "tem bytes?" (PeekNamedPipe) antes de ler.
//! No Unix o socket full-duplex não tem o problema; usamos UnixStream
//! não-bloqueante com o mesmo loop de polling (uniformiza o motor).

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

/// Comando da UI → motor.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Abre arquivo (resume em segundos, se houver) com volume inicial.
    Open {
        path: String,
        resume: Option<f64>,
        volume: f64,
    },
    Pause,
    Unpause,
    SeekAbsolute(f64),
    SeekRelative(f64),
    Volume(f64),
    Stop,
}

/// Evento do motor → UI.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// Progresso/tempo total (segundos; NaN quando só um dos dois muda).
    Time(f64, f64),
    /// Fim da faixa (keep-open: mpv fica parado no fim; a playlist decide).
    EndFile,
    /// O processo do mpv morreu/foi fechado pelo usuário.
    Exited,
    /// IPC conectada (o arquivo abriu e o motor está de pé).
    Ready,
}

/// Sobe o motor. Devolve as duas pontas da UI.
pub fn spawn() -> (Sender<Cmd>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<Event>();
    std::thread::Builder::new()
        .name("mpv-engine".into())
        .spawn(move || engine(cmd_rx, ev_tx))
        .expect("spawn mpv-engine");
    (cmd_tx, ev_rx)
}

// ── IPC por plataforma ────────────────────────────────────────────────

#[cfg(windows)]
mod ipc {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    };
    use windows::Win32::System::Pipes::PeekNamedPipe;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct Ipc {
        file: File,
    }

    impl Ipc {
        pub fn connect(path: &str) -> Result<Self, String> {
            unsafe {
                let h = CreateFileW(
                    PCWSTR::from_raw(wide(path).as_ptr()),
                    (GENERIC_READ | GENERIC_WRITE).0,
                    windows::Win32::Storage::FileSystem::FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED, // não-bloqueante (polling anti-deadlock)
                    None,
                )
                .map_err(|e| format!("abrir pipe: {e}"))?;                Ok(Self {
                    file: File::from_raw_handle(h.0 as _),
                })
            }
        }

        pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
            self.file.write_all(line.as_bytes())?;
            self.file.write_all(b"\n")
        }

        /// Lê o que houver (0 = nada novo). Err = pipe morto.
        pub fn read_available(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            unsafe {
                let h = HANDLE(self.file.as_raw_handle() as _);
                let mut avail: u32 = 0;
                PeekNamedPipe(h, None, 0, None, Some(&mut avail), None)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                if avail == 0 {
                    return Ok(0);
                }
                self.file.read(buf)
            }
        }
    }
}

#[cfg(unix)]
mod ipc {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    pub struct Ipc {
        sock: UnixStream,
    }

    impl Ipc {
        pub fn connect(path: &str) -> Result<Self, String> {
            let sock = UnixStream::connect(path).map_err(|e| format!("conectar: {e}"))?;
            sock.set_nonblocking(true).map_err(|e| e.to_string())?;
            Ok(Self { sock })
        }

        pub fn write_line(&mut self, line: &str) -> std::io::Result<()> {
            self.sock.write_all(line.as_bytes())?;
            self.sock.write_all(b"\n")
        }

        pub fn read_available(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.sock.read(buf) {
                // WouldBlock = nada novo no modo não-bloqueante.
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
                other => other,
            }
        }
    }
}

#[cfg(windows)]
use ipc::Ipc;
#[cfg(unix)]
use ipc::Ipc;

fn ipc_addr() -> String {
    if cfg!(windows) {
        format!(r"\\.\pipe\labplayer-mpv-{}", std::process::id())
    } else {
        std::env::temp_dir()
            .join(format!("labplayer-mpv-{}.sock", std::process::id()))
            .display()
            .to_string()
    }
}

// ── o motor (uma thread, dona de tudo) ────────────────────────────────

fn engine(cmd_rx: Receiver<Cmd>, ev_tx: Sender<Event>) {
    let mut child: Option<Child> = None;
    let mut ipc: Option<Ipc> = None;
    let mut line_buf: Vec<u8> = Vec::new();
    let mut read_buf = [0u8; 4096];

    loop {
        // 1) comandos da UI (drena tudo; o último é o que vale em seek).
        while let Ok(cmd) = cmd_rx.try_recv() {
            apply(&cmd, &mut child, &mut ipc, &ev_tx);
        }

        // 2) eventos do mpv (polling; linha por linha).
        if let Some(i) = ipc.as_mut() {
            loop {
                match i.read_available(&mut read_buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        line_buf.extend_from_slice(&read_buf[..n]);
                        while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                            let line: Vec<u8> = line_buf.drain(..=pos).collect();
                            let text = String::from_utf8_lossy(&line).to_string();
                            if let Some(ev) = parse_event(&text) {
                                let _ = ev_tx.send(ev);
                            }
                        }
                    }
                }
            }
        }

        // 3) mpv morreu?
        if let Some(c) = child.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                let _ = ev_tx.send(Event::Exited);
                child = None;
                ipc = None;
            }
        }

        // 4) espera: com IPC viva o tick é rápido (a seek bar depende dos
        //    eventos); parado, dorme mais.
        let _ = cmd_rx.recv_timeout(if ipc.is_some() {
            Duration::from_millis(80)
        } else {
            Duration::from_millis(300)
        });
    }
}

fn apply(cmd: &Cmd, child: &mut Option<Child>, ipc: &mut Option<Ipc>, ev_tx: &Sender<Event>) {
    let send = |ipc: &mut Option<Ipc>, json: &str| {
        if let Some(i) = ipc.as_mut() {
            let _ = i.write_line(json);
        }
    };
    match cmd {
        Cmd::Open { path, resume, volume } => {
            kill(child, ipc);
            let addr = ipc_addr();
            if !cfg!(windows) {
                let _ = std::fs::remove_file(&addr);
            }
            let mut args: Vec<String> = vec![
                format!("--input-ipc-server={addr}"),
                format!("--volume={volume}"),
                "--force-window=yes".into(),
                "--keep-open=always".into(),
            ];
            if let Some(t) = resume.filter(|t| *t > 5.0) {
                args.push(format!("--start={t}"));
            }
            args.push(path.clone());

            match Command::new(mpv_bin())
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => {
                    *child = Some(c);
                    // A IPC nasce junto com o mpv — paciência pro connect.
                    for _ in 0..50 {
                        if let Ok(mut i) = Ipc::connect(&addr) {
                            for prop in ["time-pos", "duration"] {
                                let _ = i.write_line(&format!(
                                    r#"{{"command":["observe_property",0,"{prop}"]}}"#
                                ));
                            }
                            *ipc = Some(i);
                            let _ = ev_tx.send(Event::Ready);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                Err(e) => {
                    eprintln!("mpv não iniciou: {e}");
                    let _ = ev_tx.send(Event::Exited);
                }
            }
        }
        Cmd::Pause => send(ipc, r#"{"command":["set_property","pause",true]}"#),
        Cmd::Unpause => send(ipc, r#"{"command":["set_property","pause",false]}"#),
        Cmd::SeekAbsolute(t) => {
            send(ipc, &format!(r#"{{"command":["seek",{t},"absolute"]}}"#))
        }
        Cmd::SeekRelative(d) => {
            send(ipc, &format!(r#"{{"command":["seek",{d},"relative"]}}"#))
        }
        Cmd::Volume(v) => send(
            ipc,
            &format!(r#"{{"command":["set_property","volume",{v}]}}"#),
        ),
        Cmd::Stop => kill(child, ipc),
    }
}

fn kill(child: &mut Option<Child>, ipc: &mut Option<Ipc>) {
    if let Some(i) = ipc.as_mut() {
        let _ = i.write_line(r#"{"command":["quit"]}"#);
    }
    if let Some(mut c) = child.take() {
        std::thread::sleep(Duration::from_millis(150));
        let _ = c.kill();
        let _ = c.wait();
    }
    *ipc = None;
}

/// Windows: `mpv.exe` (PATH ou ao lado do exe — o oficial embute do espelho
/// Local-runtimes; no lab basta estar no PATH). Linux: `mpv` do PATH.
fn mpv_bin() -> &'static str {
    if cfg!(windows) {
        "mpv.exe"
    } else {
        "mpv"
    }
}

fn parse_event(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("event")?.as_str()? {
        "property-change" => {
            let name = v.get("name")?.as_str()?;
            let data = v.get("data").cloned().unwrap_or(serde_json::Value::Null);
            match name {
                "time-pos" => Some(Event::Time(data.as_f64()?, f64::NAN)),
                "duration" => Some(Event::Time(f64::NAN, data.as_f64()?)),
                _ => None,
            }
        }
        "end-file" => Some(Event::EndFile),
        _ => None,
    }
}
