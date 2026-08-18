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

1. **Onda 1 (esta):** `lab-ui` (tema/i18n/config — esqueleto do "padrão egui")
   + `lab-monitor` (CPU/memória/núcleos ao vivo) + `lab-calc` (expressões,
   preview ao vivo, histórico).
2. **Onda 2:** `lab-clip` (tray + hotkey global + poller de clipboard — aqui
   começa a cópia real de módulos Rust do LocalClip) e tabela de processos no
   `lab-monitor`.
3. **Onda 3:** `lab-keys` (cofre `.tkeys` real, só UI nova) ou
   `lab-converter` (fila + progresso).

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
```

Testes: `cargo test --workspace` (o CI roda).
