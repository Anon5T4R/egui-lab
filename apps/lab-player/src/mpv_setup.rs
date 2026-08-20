//! Download e setup do mpv portátil — sem instalação externa.
//!
//! O player é um remote control: spawn `mpv.exe` e controla via IPC.
//! Para não depender de instalação do sistema, baixamos o mpv portátil
//! na primeira execução e guardamos em `%LOCALAPPDATA%\LabSuite\mpv\`.

use std::path::{Path, PathBuf};

/// Versão do mpv que baixamos. Atualizar quando necessário.
/// Fonte: https://sourceforge.net/projects/mpv-player-windows/files/64bit/
const MPV_VERSION: &str = "2025-03-09";
const MPV_ARCHIVE_NAME: &str = "mpv-x86_64-2025-03-09-git-0c144a5.7z";
const MPV_URL: &str = "https://sourceforge.net/projects/mpv-player-windows/files/64bit/mpv-x86_64-2025-03-09-git-0c144a5.7z/download";

/// Diretório onde guardamos o mpv portátil.
fn mpv_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.local/share")))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("LabSuite").join("mpv")
}

/// Path completo do executável do mpv.
pub fn mpv_path() -> PathBuf {
    mpv_dir().join(if cfg!(windows) { "mpv.exe" } else { "mpv" })
}

/// Status do setup do mpv.
pub enum MpvStatus {
    /// mpv já está pronto pra uso.
    Ready,
    /// mpv precisa ser baixado. Mensagem descritiva para o UI.
    NeedsDownload(String),
    /// Erro irrecuperável.
    Error(String),
}

/// Verifica se o mpv está disponível. Se não, retorna NeedsDownload.
pub fn check() -> MpvStatus {
    let path = mpv_path();
    if path.exists() {
        return MpvStatus::Ready;
    }

    // Verifica se 7z está disponível (necessário pra extrair)
    let has_7z = std::process::Command::new("7z")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
        || std::process::Command::new("7zz")
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        || std::process::Command::new("7za")
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok();

    if !has_7z {
        return MpvStatus::Error(
            "7-Zip não encontrado. Instale: winget install 7zip.7zip".into(),
        );
    }

    MpvStatus::NeedsDownload(format!(
        "mpv não encontrado em {}. Será baixado automaticamente (~80 MB).",
        path.display()
    ))
}

/// Baixa e extrai o mpv portátil. Bloqueante — chamar em thread separada.
/// Retorna o path do executável ou erro.
pub fn download() -> Result<PathBuf, String> {
    let dir = mpv_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("criar pasta: {e}"))?;

    let archive_path = dir.join(MPV_ARCHIVE_NAME);

    // 1) Baixa o .7z
    eprintln!("[lab-player] baixando mpv {MPV_VERSION}...");
    download_file(MPV_URL, &archive_path)?;

    // 2) Extrai com 7z (portátil) ou powershell
    eprintln!("[lab-player] extraindo...");
    extract_7z(&archive_path, &dir)?;

    // 3) Move o mpv.exe da subpasta pra raiz (o zip tem uma pasta interna)
    let exe = find_mpv_exe(&dir)?;
    let target = mpv_path();
    if exe != target {
        std::fs::copy(&exe, &target).map_err(|e| format!("copiar mpv.exe: {e}"))?;
    }

    // 4) Limpa o archive e a pasta temporária
    let _ = std::fs::remove_file(&archive_path);
    let _ = std::fs::remove_dir_all(dir.join("mpv"));

    eprintln!("[lab-player] mpv pronto: {}", target.display());
    Ok(target)
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(300))
        .redirects(10)
        .build();

    let resp = agent.get(url).call().map_err(|e| format!("download: {e}"))?;

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("criar arquivo: {e}"))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("salvar: {e}"))?;

    Ok(())
}

/// Extrai um .7z usando powershell (Expand-Archive não suporta 7z).
/// Fallback: tenta 7z se disponível, senão erro.
fn extract_7z(archive: &Path, dest: &Path) -> Result<(), String> {
    // Tenta 7z se disponível
    if let Ok(status) = std::process::Command::new("7z")
        .args(["x", archive.to_str().unwrap(), &format!("-o{}", dest.display()), "-y"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    // Tenta 7zz (versão unificada do p7zip)
    if let Ok(status) = std::process::Command::new("7zz")
        .args(["x", archive.to_str().unwrap(), &format!("-o{}", dest.display()), "-y"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    // Tenta 7za (popular no Windows via scoop/choco)
    if let Ok(status) = std::process::Command::new("7za")
        .args(["x", archive.to_str().unwrap(), &format!("-o{}", dest.display()), "-y"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    // Nenhum 7z disponível — tenta instalar via winget/scoop como fallback
    Err("7z não encontrado. Instale 7-Zip (winget install 7zip.7zip) ou baixe o mpv manualmente.".into())
}

/// Procura o mpv.exe na árvore extraída.
fn find_mpv_exe(dir: &Path) -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) { "mpv.exe" } else { "mpv" };

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
