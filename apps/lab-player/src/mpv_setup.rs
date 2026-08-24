//! Download e setup do mpv portátil — sem instalação externa.
//!
//! O player é um remote control: spawn `mpv.exe` e controla via IPC.
//! Para não depender de instalação do sistema, baixamos o mpv portátil
//! na primeira execução e guardamos em `%LOCALAPPDATA%\LabSuite\mpv\`.
//!
//! Extração: `sevenz-rust2` (Rust puro — sem 7z, sem dependência externa).

use std::path::{Path, PathBuf};

#[cfg(windows)]
use sevenz_rust2::{ArchiveReader, Password};

/// Versão do mpv que baixamos. Atualizar quando necessário.
/// Fonte: https://sourceforge.net/projects/mpv-player-windows/files/64bit/
#[cfg(windows)]
const MPV_ARCHIVE_NAME: &str = "mpv-x86_64-20260809-git-dd5d17d328.7z";
#[cfg(windows)]
const MPV_URL: &str = "https://sourceforge.net/projects/mpv-player-windows/files/64bit/mpv-x86_64-20260809-git-dd5d17d328.7z/download";

/// Diretório onde guardamos o mpv portátil (Windows).
#[cfg(windows)]
fn mpv_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("LabSuite").join("mpv")
}

/// Path completo do executável do mpv (Windows: LabSuite; Linux: PATH).
pub fn mpv_path() -> PathBuf {
    if cfg!(windows) {
        mpv_dir().join("mpv.exe")
    } else {
        PathBuf::from("mpv")
    }
}

/// Status do setup do mpv.
pub enum MpvStatus {
    /// mpv já está pronto pra uso.
    Ready,
    /// mpv precisa ser baixado. Mensagem descritiva para o UI.
    NeedsDownload(String),
    /// Erro irrecuperável (Linux sem mpv no PATH).
    #[allow(dead_code)]
    Error(String),
}

/// Verifica se o mpv está disponível.
/// Windows: cópia portátil em LabSuite (baixa se faltar).
/// Linux: mpv do PATH (pacote da distro — o download é build Windows).
pub fn check() -> MpvStatus {
    if !cfg!(windows) {
        let ok = std::process::Command::new("mpv")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        return if ok {
            MpvStatus::Ready
        } else {
            MpvStatus::Error("mpv não está no PATH — instale (ex.: sudo apt install mpv)".into())
        };
    }

    let path = mpv_path();
    if path.exists() {
        return MpvStatus::Ready;
    }

    MpvStatus::NeedsDownload("mpv não encontrado. Será baixado automaticamente (~80 MB).".into())
}

/// Baixa e extrai o mpv portátil (Windows). Bloqueante — chamar em thread
/// separada. Retorna o path do executável ou erro.
#[cfg(windows)]
pub fn download() -> Result<PathBuf, String> {
    let dir = mpv_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("criar pasta: {e}"))?;

    let archive_path = dir.join(MPV_ARCHIVE_NAME);

    // 1) Baixa o .7z
    eprintln!("[lab-player] baixando mpv...");
    download_file(MPV_URL, &archive_path)?;

    // 2) Extrai com sevenz-rust2 (Rust puro)
    eprintln!("[lab-player] extraindo...");
    extract_7z(&archive_path, &dir)?;

    // 3) Procura o mpv.exe na árvore extraída
    let exe = find_mpv_exe(&dir)?;
    let target = mpv_path();
    if exe != target {
        std::fs::copy(&exe, &target).map_err(|e| format!("copiar mpv.exe: {e}"))?;
    }

    // 4) Limpa o archive
    let _ = std::fs::remove_file(&archive_path);

    eprintln!("[lab-player] mpv pronto: {}", target.display());
    Ok(target)
}

#[cfg(windows)]
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(300))
        .redirects(10)
        .build();

    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("download: {e}"))?;

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("criar arquivo: {e}"))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("salvar: {e}"))?;

    Ok(())
}

/// Extrai um .7z usando sevenz-rust2 (Rust puro).
#[cfg(windows)]
fn extract_7z(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| format!("abrir archive: {e}"))?;

    let mut reader =
        ArchiveReader::new(file, Password::empty()).map_err(|e| format!("abrir 7z: {e}"))?;

    // 7z costuma ser sólido: for_each_entries decodifica em sequência.
    reader
        .for_each_entries(|entry, rd| {
            let name = entry.name.clone();
            let is_dir = entry.is_directory;

            if name.is_empty() {
                return Ok(true);
            }

            let target = dest.join(&name);

            if is_dir {
                let _ = std::fs::create_dir_all(&target);
                return Ok(true);
            }

            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let mut out = match std::fs::File::create(&target) {
                Ok(f) => f,
                Err(_) => return Ok(true), // pula arquivos que não consegue criar
            };

            let mut buf = vec![0u8; 512 * 1024];
            loop {
                let n = match rd.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                let _ = std::io::Write::write_all(&mut out, &buf[..n]);
            }

            Ok(true)
        })
        .map_err(|e| format!("extrair 7z: {e}"))?;

    Ok(())
}

/// Procura o mpv.exe na árvore extraída.
#[cfg(windows)]
fn find_mpv_exe(dir: &Path) -> Result<PathBuf, String> {
    let exe_name = "mpv.exe";

    // Procura na raiz
    if dir.join(exe_name).exists() {
        return Ok(dir.join(exe_name));
    }

    // Procura em subpastas (1 nível)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path().join(exe_name);
            if p.exists() {
                return Ok(p);
            }
        }
    }

    Err(format!(
        "mpv.exe não encontrado após extração em {}",
        dir.display()
    ))
}
