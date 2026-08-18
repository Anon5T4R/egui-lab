# egui-lab

Bancada de avaliação de **egui/eframe** para a suíte Local. Portamos candidatos
a "app-ferramenta" pra cá para medir com dado real o que a análise apontou
(2026-08-18): egui não substitui o Tauri da suíte, mas é forte em apps
orientados a dado (listas, gráficos, tempo real) com integração OS via crates.

Nada daqui vira app da suíte sem decisão explícita; app que "se formar" ganha
repo próprio seguindo um futuro padrão egui.

## Regras do lab

- Os repos oficiais (`C:\Dev\Local\*`) **não são mexidos**. Os pilotos da onda 1
  são reescritas de referência; a partir da onda 2 copiamos módulos Rust reais
  dos apps (a prova de reuso de back-end).
- CI é o juiz (`cargo check` + `cargo test`, Windows e Ubuntu) — sem build
  local, como no resto da suíte.
- `Cargo.lock` commitado; commits em PT, sem Co-Authored-By.
- Sem porta de dev (não há webview/HMR) — só `cargo run -p <crate>`. A
  tabela de portas em `dev-notes/docs/ESTADO.md` não ganha entrada.

## Candidatos (ranqueados na análise de 2026-08-18)

### Tier S — portar primeiro

| App | Por quê | O que testa no egui |
|---|---|---|
| **LocalMonitor** | o app inteiro é `sysinfo` + apresentação; sparklines viram pintura direta; tempo real é o modo natural do immediate mode | repaint contínuo, gráficos à mão, tabela de processos (onda 2) |
| **LocalCalc** | UI mínima → valida o port do padrão (tema 5 paletas + i18n PT/EN/ES + config) pelo menor custo | motor de expressão, forms, teclado |
| **LocalClip** | lista + busca + preview; a parte difícil (poller, hotkey global, tray) é integração OS via crates | `tray-icon`, `global-hotkey`, `arboard`, SQLite |
| **LocalKeys** | back 100% Rust (XChaCha20-Poly1305 + Argon2id, `.tkeys` validado contra o Android) reaproveitável | forms densos, keyring, cripto em repouso |

### Tier A — viáveis com ressalva

- **LocalConverter** — fila em lote + progresso (orquestração ffmpeg/pandoc já
  é Rust); drag & drop de arquivo no egui é mais cru que na web.
- **LocalZip** — árvore + progresso; pouco a testar além do que Monitor/Clip
  já cobrem.
- **LocalData** — `egui_extras::Table` aguenta tabela grande, mas edição e
  fórmulas complicam.
- **LocalAutomation** — node graph existe em egui, mas o editor visual é
  reescrita grande.
- **LocalTranslate** — candle já é Rust (ganharia sem IPC), mas o textarea
  multilinha do egui é mediano para textos longos.

### Não levar pro lab

Office, PDF, Sheets, Slides, Code, Paint, Draw, Video, Media, Player, Scribe,
ZIM, Feed, TaylorChat, Agenda — rich text, DOM, canvas pesado ou layout de
leitura: o terreno onde o webview ganha com folga.

## Ondas

1. **Onda 1 (feita):** `lab-ui` (tema/i18n/config — esqueleto do "padrão egui")
   + `lab-monitor` (CPU/memória/núcleos ao vivo) + `lab-calc` (expressões,
   preview ao vivo, histórico).
2. **Onda 2 (feita):** `lab-clip` — bandeja (`tray-icon`), atalho global
   **Ctrl+Alt+V** (`global-hotkey`; o oficial é Ctrl+Shift+V e dois apps não
   registram o mesmo atalho — o LocalClip instalado é dono), poller de texto
   (`arboard`, 800 ms, dedup) com a flag
   `ExcludeClipboardContentFromMonitorProcessing` copiada do oficial (senha
   copiada do LocalKeys NÃO entra no histórico). Só texto, histórico em
   memória (sem SQLite — escopo). Tabela de processos no `lab-monitor`
   (filtro/ordenação/encerrar com confirmação).
3. **Onda 3 (feita):** `lab-keys` — abre/cria um cofre `.tkeys` **REAL**:
   `crypto.rs` (XChaCha20-Poly1305 + Argon2id), a gravação atômica e o
   `copy_secret` (formatos de exclusão do Windows + limpeza em 30 s) são
   copiados verbatim do LocalKeys@0.9.0. Escopo: destrancar, listar, buscar,
   copiar senha/usuário, acrescentar login (escrita conservadora — só
   acrescenta; pastas/anexos/campos custom atravessam intactos), trancar
   (botão ou ao minimizar, regra do oficial).

## Releases

Tag `vX.Y.Z` → `release.yml` publica na GitHub Release:

- **Windows:** `lab-<app>-windows-x64.zip` — `.exe` com **CRT estático**
  (`.cargo/config.toml`): roda em qualquer Windows, sem VC++ redistributable.
- **Linux:** `lab-<app>-x86_64.AppImage` — linuxdeploy + o reempacotamento da
  suíte (`Anon5T4R/linux-packaging/fix-appimage@v1`: libs de GPU/Wayland do
  host, type2-runtime/fuse3).

Gotchas conhecidos do lab: `lab-keys` roda o Argon2 na thread da UI (~300 ms
de freeze no destrancar — o oficial usa command async); `lab-clip` encerra no
X (o "fechar pra bandeja" do oficial é opt-in e não configuramos); diálogos de
arquivo nativos só no Windows (rfd win32) — no Linux o caminho do `.tkeys` é
digitado, de propósito (gtk3 no AppImage engordaria dezenas de MB à toa).

## Critérios de decisão (go/no-go)

1. Polimento visual alcança o padrão Tauri da suíte (temas nomeados incluídos)?
2. Integração OS (tray/hotkey/autostart) sem duct tape?
3. RAM e startup **medidos** contra o app Tauri equivalente — números, não
   achismo.
4. Manter duas toolchains + dois padrões de UI cabe no custo da suíte?
5. Acessibilidade (AccessKit) suficiente para o público dos apps-ferramenta?

## Rodar

```
cargo run -p lab-monitor
cargo run -p lab-calc
cargo run -p lab-clip
cargo run -p lab-keys
```

Testes: `cargo test --workspace` (o CI roda).
