//! Instalação do lab-hub: onde os apps moram, download das releases e
//! registro do que está instalado.
//!
//! **Onde armazena os executáveis:** `%LOCALAPPDATA%\LabSuite\<app>\` no
//! Windows e `~/.local/share/LabSuite/<app>/` no Linux — LOCAL, não roaming:
//! binário não é dado de usuário itinerante (mesma razão do cache). O hub
//! e o `installed.json` (com as versões) ficam na raiz `LabSuite\`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

/// Catálogo fixo — monorepo: UMA tag/release serve para todos os apps.
pub struct AppDef {
    /// Nome do crate/binário (também é o id de tudo: pasta, ícone, assets).
    pub id: &'static str,
    /// Nome de exibição.
    pub display: &'static str,
    /// Asset da release para Windows.
    pub win_asset: &'static str,
    /// Asset da release para Linux (nome que o linuxdeploy gera).
    pub linux_asset: &'static str,
    /// Ícone REAL do irmão Tauri (repo público da suíte).
    pub icon_ico_url: &'static str,
    /// Ícone PNG 128 (Linux + textura do card; no Windows o atalho usa o
    /// `.ico`). Só é lido fora do Windows hoje — mantido no catálogo porque
    /// descreve o par de ícones do app por completo.
    #[cfg_attr(windows, allow(dead_code))]
    pub icon_png_url: &'static str,
}

pub const APPS: &[AppDef] = &[
    AppDef {
        id: "lab-monitor",
        display: "Lab Monitor",
        win_asset: "lab-monitor-windows-x64.zip",
        linux_asset: "Lab_Monitor-x86_64.AppImage",
        icon_ico_url: "https://raw.githubusercontent.com/Anon5T4R/LocalMonitor/main/src-tauri/icons/icon.ico",
        icon_png_url: "https://raw.githubusercontent.com/Anon5T4R/LocalMonitor/main/src-tauri/icons/128x128.png",
    },
    AppDef {
        id: "lab-calc",
        display: "Lab Calc",
        win_asset: "lab-calc-windows-x64.zip",
        linux_asset: "Lab_Calc-x86_64.AppImage",
        icon_ico_url: "https://raw.githubusercontent.com/Anon5T4R/LocalCalc/main/src-tauri/icons/icon.ico",
        icon_png_url: "https://raw.githubusercontent.com/Anon5T4R/LocalCalc/main/src-tauri/icons/128x128.png",
    },
    AppDef {
        id: "lab-clip",
        display: "Lab Clip",
        win_asset: "lab-clip-windows-x64.zip",
        linux_asset: "Lab_Clip-x86_64.AppImage",
        icon_ico_url: "https://raw.githubusercontent.com/Anon5T4R/LocalClip/main/src-tauri/icons/icon.ico",
        icon_png_url: "https://raw.githubusercontent.com/Anon5T4R/LocalClip/main/src-tauri/icons/128x128.png",
    },
    AppDef {
        id: "lab-keys",
        display: "Lab Keys",
        win_asset: "lab-keys-windows-x64.zip",
        linux_asset: "Lab_Keys-x86_64.AppImage",
        icon_ico_url: "https://raw.githubusercontent.com/Anon5T4R/LocalKeys/main/src-tauri/icons/icon.ico",
        icon_png_url: "https://raw.githubusercontent.com/Anon5T4R/LocalKeys/main/src-tauri/icons/128x128.png",
    },
];

pub const REPO: &str = "Anon5T4R/egui-lab";
const RELEASES_LATEST: &str = "https://api.github.com/repos/Anon5T4R/egui-lab/releases/latest";

/// O próprio hub — card separado (não entra em APPS: não tem "instalar" nem
/// "desinstalar", é atualização de si mesmo com o exe rodando).
pub const HUB: AppDef = AppDef {
    id: "lab-hub",
    display: "Lab Hub",
    win_asset: "lab-hub-windows-x64.zip",
    linux_asset: "Lab_Hub-x86_64.AppImage",
    icon_ico_url: "https://raw.githubusercontent.com/Anon5T4R/egui-lab/main/apps/lab-hub/icon.ico",
    icon_png_url: "https://raw.githubusercontent.com/Anon5T4R/egui-lab/main/icons/lab-hub.png",
};

// ── onde as coisas moram ──────────────────────────────────────────────

/// Raiz da instalação (`%LOCALAPPDATA%\LabSuite` no Windows,
/// `~/.local/share/LabSuite` no Linux).
pub fn install_root() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(l) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(l).join("LabSuite");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(x) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(x).join("LabSuite");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/LabSuite")
}

pub fn app_dir(id: &str) -> PathBuf {
    install_root().join(id)
}

/// Ícone local do app (baixado na instalação; ext erra por plataforma).
pub fn icon_path(id: &str) -> PathBuf {
    #[cfg(windows)]
    {
        install_root().join("icons").join(format!("{id}.ico"))
    }
    #[cfg(not(windows))]
    {
        install_root().join("icons").join(format!("{id}.png"))
    }
}

// ── registro do instalado ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct InstalledApp {
    /// Tag da release instalada (ex.: "v0.2.2").
    pub version: String,
    /// Caminho completo do executável/AppImage.
    pub exe: String,
}

fn installed_json() -> PathBuf {
    install_root().join("installed.json")
}

pub fn load_installed() -> std::collections::HashMap<String, InstalledApp> {
    std::fs::read_to_string(installed_json())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_installed(map: &std::collections::HashMap<String, InstalledApp>) {
    let p = installed_json();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(p, json);
    }
}

// ── rede ──────────────────────────────────────────────────────────────

/// Tag da última release (`None` = rede/limite de rate fora do ar).
pub fn fetch_latest_tag() -> Result<String, String> {
    let resp = ureq::get(RELEASES_LATEST)
        .set("User-Agent", "lab-hub")
        .call()
        .map_err(|e| format!("releases/latest: {e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("json da release: {e}"))?;
    v.get("tag_name")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "release sem tag_name".into())
}

fn asset_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{asset}")
}

/// Baixa `url` em `dest` reportando progresso (0.0–1.0) quando o tamanho é
/// conhecido. É o corpo comum de tudo (assets e ícones).
fn download_to(url: &str, dest: &Path, tx: &Sender<f32>) -> Result<(), String> {
    let resp = ureq::get(url)
        .set("User-Agent", "lab-hub")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let total: Option<u64> = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok());
    let mut reader = resp.into_reader();
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?,
    );
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if let Some(t) = total {
            let _ = tx.send((done as f32 / t as f32).clamp(0.0, 1.0));
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// ── instalação ────────────────────────────────────────────────────────

/// Extrai o zip da release (um .exe na raiz) e devolve o caminho dele.
/// Compila nas duas plataformas (o zip é crate multiplataforma); só é CHAMADO
/// no Windows — assim o CI do Linux também valida este código.
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut arc = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let mut exe: Option<PathBuf> = None;
    for i in 0..arc.len() {
        let mut entry = arc.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // Nome sanitizado (o zip é nosso, mas zip-slip é hábito).
        let name = entry.name().replace('\\', "/");
        let Some(file_name) = name.rsplit('/').next() else {
            continue;
        };
        let out = dest_dir.join(file_name);
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
        if file_name.ends_with(".exe") {
            exe = Some(out);
        }
    }
    exe.ok_or_else(|| "zip sem .exe".to_string())
}

/// Instala `app` na `tag`: download → staging → swap → ícone → registro.
/// Progresso 0–1 vai pelo canal; `installed` é atualizado em caso de sucesso.
pub fn install_app(
    app: &AppDef,
    tag: &str,
    installed: &mut std::collections::HashMap<String, InstalledApp>,
    tx: &Sender<f32>,
) -> Result<(), String> {
    let root = install_root();
    let staging = root.join(".staging").join(app.id);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let asset = if cfg!(windows) { app.win_asset } else { app.linux_asset };
    let zip_path = staging.join(asset);
    download_to(&asset_url(tag, asset), &zip_path, tx)?;
    let _ = tx.send(0.95);

    let target = app_dir(app.id);
    let _ = std::fs::remove_dir_all(&target); // exe em uso vai dar erro abaixo — justo
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    let exe = if cfg!(windows) {
        extract_zip(&zip_path, &target)?
    } else {
        let out = target.join(asset);
        std::fs::rename(&zip_path, &out).map_err(|e| e.to_string())?;
        make_executable(&out)?;
        out
    };

    // Ícone (do irmão Tauri — falha é silenciosa: atalho usa o default do
    // exe). Windows: .ico pro .lnk; Linux: PNG pro .desktop.
    #[cfg(windows)]
    let icon_url = app.icon_ico_url;
    #[cfg(not(windows))]
    let icon_url = app.icon_png_url;
    let _ = download_to(icon_url, &icon_path(app.id), tx);

    installed.insert(
        app.id.to_string(),
        InstalledApp {
            version: tag.to_string(),
            exe: exe.display().to_string(),
        },
    );
    save_installed(installed);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = tx.send(1.0);
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Baixa o ícone do app se ainda não existir localmente (o hub faz pra SI
/// MESMO no boot — os outros baixam na instalação).
pub fn fetch_icon_file(app: &AppDef) -> Result<PathBuf, String> {
    let dest = icon_path(app.id);
    if dest.exists() {
        return Ok(dest);
    }
    #[cfg(windows)]
    let url = app.icon_ico_url;
    #[cfg(not(windows))]
    let url = app.icon_png_url;
    let (tx, _rx) = std::sync::mpsc::channel();
    download_to(url, &dest, &tx)?;
    Ok(dest)
}

/// Auto-update do hub com o exe RODANDO: baixa a nova versão, renomeia o
/// atual pra `.old` (Windows e Linux permitem renomear binário em execução),
/// copia o novo pro lugar (copy, não rename — o hub pode morar fora do
/// volume do LabSuite) e deixa o `.old` pra ser limpo no próximo boot.
/// O usuário reinicia o hub pra efetivar.
pub fn update_self(tag: &str, tx: &Sender<f32>) -> Result<(), String> {
    let staging = install_root().join(".staging").join("lab-hub");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let asset = if cfg!(windows) { HUB.win_asset } else { HUB.linux_asset };
    let zip_path = staging.join(asset);
    download_to(&asset_url(tag, asset), &zip_path, tx)?;
    let _ = tx.send(0.95);

    let new_exe = if cfg!(windows) {
        extract_zip(&zip_path, &staging)?
    } else {
        make_executable(&zip_path)?;
        zip_path
    };

    let current = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let old = current.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&current, &old).map_err(|e| format!("renomear atual: {e}"))?;
    if let Err(e) = std::fs::copy(&new_exe, &current) {
        // volta o antigo pro lugar — sem deixar o usuário a pé
        let _ = std::fs::rename(&old, &current);
        return Err(format!("copiar novo: {e}"));
    }
    let _ = std::fs::remove_file(&new_exe);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = tx.send(1.0);
    Ok(())
}

/// Remove o `.old` de um auto-update anterior (chamado no boot).
pub fn cleanup_self_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

// ── desinstalar / limpeza ─────────────────────────────────────────────

/// Remove o app (pasta, registro, ícone e ATALHOS — o " uninstall" do irmão
/// TaylorHub/NSIS também limpa os atalhos que ele criou).
pub fn uninstall_app(
    app: &AppDef,
    installed: &mut std::collections::HashMap<String, InstalledApp>,
) -> Result<(), String> {
    std::fs::remove_dir_all(app_dir(app.id)).map_err(|e| format!("{}: {e}", app_dir(app.id).display()))?;
    installed.remove(app.id);
    save_installed(installed);
    let _ = crate::shortcut::remove(app.id, app.display, crate::shortcut::Where::StartMenu);
    let _ = crate::shortcut::remove(app.id, app.display, crate::shortcut::Where::Desktop);
    let _ = std::fs::remove_file(icon_path(app.id));
    Ok(())
}

/// Limpeza: staging de instalações abortadas e pastas órfãs (dir existe mas
/// não tem entrada no installed.json — instalação que morreu no meio).
/// Devolve quantos itens saíram.
pub fn cleanup(installed: &std::collections::HashMap<String, InstalledApp>) -> usize {
    let mut n = 0;
    let staging = install_root().join(".staging");
    if std::fs::remove_dir_all(&staging).is_ok() {
        n += 1;
    }
    for app in APPS {
        let dir = app_dir(app.id);
        if dir.exists() && !installed.contains_key(app.id) {
            if std::fs::remove_dir_all(&dir).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Abre a pasta de instalação no gerenciador de arquivos do SO.
pub fn open_install_folder() {
    let root = install_root();
    let _ = std::fs::create_dir_all(&root);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(root).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(root).spawn();
}
