//! Importadores de outros gerenciadores → itens do vault.
//!
//! Porte fiel de `LocalKeys · src/import.ts` (o schema dos itens vive no
//! `vault.rs`). Formatos:
//!
//! - Bitwarden JSON (export não-cifrado — ou já decifrado pelo `bwdecrypt`).
//! - CSV (Chrome/Edge, LastPass, 1Password e genérico) por mapeamento de colunas.
//! - KeePass `.kdbx` — a leitura/decifragem é do `kdbx.rs`; aqui só mapeamos.
//!
//! ESCOPO: o lab só gerencia itens de LOGIN e NOTE. Card e identity do
//! Bitwarden NÃO são importados — são contados em `skipped_card_identity` e
//! descartados, porque o lab não tem UI de edição para eles (o `.tkeys` de
//! origem continua intacto; nada se perde lá).

use serde_json::Value;

use crate::kdbx::KdbxEntry;
use crate::vault::{custom_field, make_item, LoginFields, NewItem};

/// Formato detectado a partir do nome do arquivo (ou do conteúdo, sem extensão
/// conhecida) — mesma regra do `detectFormat` do TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    BitwardenJson,
    Csv,
    Kdbx,
    Unknown,
}

pub fn detect_format(filename: &str, content: Option<&str>) -> ImportFormat {
    let lower = filename.to_lowercase();
    if lower.ends_with(".kdbx") {
        return ImportFormat::Kdbx;
    }
    if lower.ends_with(".json") {
        return ImportFormat::BitwardenJson;
    }
    if lower.ends_with(".csv") {
        return ImportFormat::Csv;
    }
    if let Some(content) = content {
        let t = content.trim_start();
        if t.starts_with('{') || t.starts_with('[') {
            return ImportFormat::BitwardenJson;
        }
        return ImportFormat::Csv;
    }
    ImportFormat::Unknown
}

/// Contagem do que entrou e do que ficou de fora (card/identity do Bitwarden).
#[derive(Debug, Default)]
pub struct ImportReport {
    pub items: Vec<Value>,
    /// Itens de cartão/identidade que o lab não gerencia (só contados).
    pub skipped_card_identity: usize,
}

/// Extrai só o segredo base32 de um campo TOTP que pode vir como
/// `otpauth://…?secret=X` (ou `key=X&…` no KeePassXC).
///
/// `to_ascii_lowercase` preserva os offsets de byte do original — o `find` no
/// TS é feito na string em minúsculas, e o fatiamento é na original.
fn extract_secret(raw: &str) -> String {
    let r = raw.trim();
    let lower = r.to_ascii_lowercase();
    for marker in ["secret=", "key="] {
        if let Some(idx) = lower.find(marker) {
            let rest = &r[idx + marker.len()..];
            let end = rest.find('&').unwrap_or(rest.len());
            return rest[..end].to_string();
        }
    }
    r.to_string()
}

// ---------- CSV ----------

/// Detecta o separador do CSV (vírgula, ponto-e-vírgula ou tab) pela 1ª linha.
/// Empate mantém a vírgula, como a ordenação estável do TS.
fn detect_delimiter(text: &str) -> char {
    let first = text.split('\n').next().unwrap_or(text);
    let mut best = (',', 0usize);
    for d in [',', ';', '\t'] {
        let n = first.chars().filter(|c| *c == d).count();
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// Parser de CSV tolerante a aspas (incl. `""` escapado), separador variável e
/// quebras de linha DENTRO de campo — mesma máquina de estados do TS.
pub fn parse_csv(text: &str, delim: char) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text); // BOM
    let chars: Vec<char> = text.chars().collect();

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    field.push('"');
                    i += 2;
                    continue;
                }
                in_quotes = false;
                i += 1;
                continue;
            }
            field.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_quotes = true;
            i += 1;
        } else if c == delim {
            row.push(std::mem::take(&mut field));
            i += 1;
        } else if c == '\r' {
            i += 1;
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            i += 1;
        } else {
            field.push(c);
            i += 1;
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    // Linhas só com espaços/vazias não são registros.
    rows.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    rows
}

struct Cols {
    name: &'static [&'static str],
    username: &'static [&'static str],
    email: &'static [&'static str],
    password: &'static [&'static str],
    url: &'static [&'static str],
    totp: &'static [&'static str],
    notes: &'static [&'static str],
}

/// Cabeçalhos reconhecidos — idênticos ao `COLS` do TS (incl. os em pt).
const COLS: Cols = Cols {
    name: &["name", "title", "account", "item name"],
    username: &[
        "username",
        "user",
        "login_username",
        "user name",
        "login",
        "usuário",
        "usuario",
    ],
    email: &["email", "e-mail", "e mail"],
    password: &["password", "pass", "login_password", "senha"],
    url: &[
        "url",
        "uri",
        "website",
        "login_uri",
        "web site",
        "site",
        "endereço",
    ],
    totp: &[
        "totp",
        "otpauth",
        "otp",
        "one-time password",
        "verification code",
        "otp secret",
        "2fa",
    ],
    notes: &[
        "notes",
        "note",
        "extra",
        "comment",
        "comments",
        "nota",
        "observações",
    ],
};

/// Cabeçalhos de metadados que NÃO viram campo personalizado (só ruído).
const IGNORE: &[&str] = &[
    "type",
    "vault",
    "guid",
    "grouping",
    "fav",
    "favorite",
    "httprealm",
    "formactionorigin",
    "timecreated",
    "timelastused",
    "timepasswordchanged",
    "created",
    "modified",
    "reprompt",
    "android_app",
];

/// Equivalente ao regex `/pass|senha|secret|otp|totp|2fa|key|cvv|pin|seed/`
/// do TS — sem a dep `regex` só pra uma alternação de substrings.
fn is_secret_header(norm: &str) -> bool {
    [
        "pass", "senha", "secret", "otp", "totp", "2fa", "key", "cvv", "pin", "seed",
    ]
    .iter()
    .any(|k| norm.contains(k))
}

fn exact_index(norm: &[String], consumed: &mut [bool], keys: &[&str]) -> Option<usize> {
    for k in keys {
        if let Some(i) = norm.iter().position(|h| h == k) {
            if !consumed[i] {
                consumed[i] = true;
                return Some(i);
            }
        }
    }
    None
}

fn partial_index(norm: &[String], consumed: &mut [bool], keys: &[&str]) -> Option<usize> {
    for i in 0..norm.len() {
        if !consumed[i] && keys.iter().any(|k| norm[i].contains(k)) {
            consumed[i] = true;
            return Some(i);
        }
    }
    None
}

struct Idx {
    name: Option<usize>,
    email: Option<usize>,
    username: Option<usize>,
    password: Option<usize>,
    url: Option<usize>,
    totp: Option<usize>,
    notes: Option<usize>,
}

fn cell(row: &[String], idx: Option<usize>) -> &str {
    idx.and_then(|i| row.get(i)).map(|s| s.trim()).unwrap_or("")
}

/// CSV → itens de login. 1ª rodada de mapeamento é EXATA (evita "name" casar
/// com "username"); a 2ª é por substring. Nada se perde: email (se não virou
/// usuário) e colunas não mapeadas/não-ruído viram campos personalizados, com
/// `hidden` quando o nome do cabeçalho cheira a segredo.
pub fn parse_csv_items(text: &str) -> Vec<Value> {
    let rows = parse_csv(text, detect_delimiter(text));
    if rows.len() < 2 {
        return Vec::new();
    }
    let headers = &rows[0];
    let norm: Vec<String> = headers.iter().map(|h| h.trim().to_lowercase()).collect();
    let mut consumed = vec![false; norm.len()];

    let mut idx = Idx {
        name: exact_index(&norm, &mut consumed, COLS.name),
        email: exact_index(&norm, &mut consumed, COLS.email),
        username: exact_index(&norm, &mut consumed, COLS.username),
        password: exact_index(&norm, &mut consumed, COLS.password),
        url: exact_index(&norm, &mut consumed, COLS.url),
        totp: exact_index(&norm, &mut consumed, COLS.totp),
        notes: exact_index(&norm, &mut consumed, COLS.notes),
    };
    // 2ª rodada parcial — MESMA ordem do TS (email não tem rodada parcial).
    if idx.name.is_none() {
        idx.name = partial_index(&norm, &mut consumed, COLS.name);
    }
    if idx.username.is_none() {
        idx.username = partial_index(&norm, &mut consumed, COLS.username);
    }
    if idx.password.is_none() {
        idx.password = partial_index(&norm, &mut consumed, COLS.password);
    }
    if idx.url.is_none() {
        idx.url = partial_index(&norm, &mut consumed, COLS.url);
    }
    if idx.totp.is_none() {
        idx.totp = partial_index(&norm, &mut consumed, COLS.totp);
    }
    if idx.notes.is_none() {
        idx.notes = partial_index(&norm, &mut consumed, COLS.notes);
    }

    let mut items = Vec::new();
    for row in &rows[1..] {
        let email = cell(row, idx.email);
        let username = {
            let u = cell(row, idx.username);
            if u.is_empty() {
                email // usuário cai pro email
            } else {
                u
            }
        };
        let url = cell(row, idx.url);
        let name = {
            let n = cell(row, idx.name);
            if !n.is_empty() {
                n
            } else if !url.is_empty() {
                url
            } else if !username.is_empty() {
                username
            } else {
                "(sem nome)"
            }
        };
        let notes = cell(row, idx.notes);
        let totp = extract_secret(cell(row, idx.totp));

        let mut extras = Vec::new();
        if !email.is_empty() && email != username {
            extras.push(custom_field("email", email, false));
        }
        for (i, header) in headers.iter().enumerate() {
            if consumed.get(i).copied().unwrap_or(true) || IGNORE.contains(&norm[i].as_str()) {
                continue;
            }
            let value = cell(row, Some(i));
            if value.is_empty() {
                continue;
            }
            let header = header.trim();
            let field_name = if header.is_empty() {
                format!("campo {}", i + 1)
            } else {
                header.to_string()
            };
            extras.push(custom_field(&field_name, value, is_secret_header(&norm[i])));
        }

        items.push(make_item(NewItem {
            kind: "login",
            name,
            notes,
            favorite: false,
            login: Some(LoginFields {
                username: username.to_string(),
                password: cell(row, idx.password).to_string(),
                uris: if url.is_empty() {
                    vec![String::new()]
                } else {
                    vec![url.to_string()]
                },
                totp,
            }),
            custom_fields: extras,
        }));
    }
    items
}

// ---------- Bitwarden JSON ----------

/// Tipos do Bitwarden: 1=login (default), 2=nota, 3=cartão, 4=identidade.
fn bw_kind(t: Option<&Value>) -> &'static str {
    match t.and_then(Value::as_i64) {
        Some(2) => "note",
        Some(3) => "card",
        Some(4) => "identity",
        _ => "login",
    }
}

fn bitwarden_login(b: &Value) -> LoginFields {
    let login = b.get("login");
    let get = |k: &str| {
        login
            .and_then(|l| l.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    let mut uris: Vec<String> = login
        .and_then(|l| l.get("uris"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|u| u.get("uri").and_then(Value::as_str))
                .filter(|s| !s.is_empty()) // o TS filtra com filter(Boolean)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if uris.is_empty() {
        uris.push(String::new());
    }
    LoginFields {
        username: get("username").to_string(),
        password: get("password").to_string(),
        uris,
        totp: extract_secret(get("totp")),
    }
}

/// Array na raiz OU `{ "items": [...] }` (os dois formatos de export).
pub fn parse_bitwarden_json(text: &str) -> Result<ImportReport, String> {
    let data: Value = serde_json::from_str(text).map_err(|e| format!("JSON inválido: {e}"))?;
    let arr: &[Value] = if let Some(a) = data.as_array() {
        a
    } else {
        data.get("items")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    };

    let mut report = ImportReport::default();
    for b in arr {
        let kind = bw_kind(b.get("type"));
        // Decisão de escopo: o lab só gerencia login/note.
        if kind == "card" || kind == "identity" {
            report.skipped_card_identity += 1;
            continue;
        }
        let name = b
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("(sem nome)");
        let notes = b.get("notes").and_then(Value::as_str).unwrap_or("");
        let favorite = b.get("favorite").and_then(Value::as_bool).unwrap_or(false);
        report.items.push(make_item(NewItem {
            kind,
            name,
            notes,
            favorite,
            login: if kind == "login" {
                Some(bitwarden_login(b))
            } else {
                None
            },
            custom_fields: Vec::new(),
        }));
    }
    Ok(report)
}

// ---------- KeePass (.kdbx) — a leitura vem do `kdbx.rs` ----------

/// Entradas cruas do `.kdbx` → itens de login do vault.
pub fn kdbx_to_items(entries: &[KdbxEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|e| {
            let name = if !e.name.is_empty() {
                e.name.as_str()
            } else if !e.url.is_empty() {
                e.url.as_str()
            } else if !e.username.is_empty() {
                e.username.as_str()
            } else {
                "(sem nome)"
            };
            make_item(NewItem {
                kind: "login",
                name,
                notes: &e.notes,
                favorite: false,
                login: Some(LoginFields {
                    username: e.username.clone(),
                    password: e.password.clone(),
                    uris: if e.url.is_empty() {
                        vec![String::new()]
                    } else {
                        vec![e.url.clone()]
                    },
                    totp: e.totp.clone(),
                }),
                custom_fields: Vec::new(),
            })
        })
        .collect()
}

/// Import de TEXTO (JSON/CSV). Para `.kdbx` use `kdbx::import` + `kdbx_to_items`.
// Paridade com o `parseText` do TS; a UI da onda 6 chama os parsers diretos.
#[allow(dead_code)]
pub fn parse_text(
    filename: &str,
    content: &str,
) -> Result<(ImportReport, ImportFormat), String> {
    // Mesmo contrato do TS: o que não é Bitwarden-JSON cai no parser de CSV e
    // o formato devolvido é "csv" (kdbx não passa por aqui).
    if detect_format(filename, Some(content)) == ImportFormat::BitwardenJson {
        Ok((parse_bitwarden_json(content)?, ImportFormat::BitwardenJson))
    } else {
        Ok((
            ImportReport {
                items: parse_csv_items(content),
                skipped_card_identity: 0,
            },
            ImportFormat::Csv,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_com_aspas_virgula_e_quebra_dentro_do_campo() {
        let csv = "name,username,password,url,notes\n\
                   \"Gmail, pessoal\",joao,\"sen,ha\",https://gmail.com,\"linha 1\nlinha 2\"\n";
        let items = parse_csv_items(csv);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "Gmail, pessoal");
        assert_eq!(items[0]["login"]["username"], "joao");
        assert_eq!(items[0]["login"]["password"], "sen,ha");
        assert_eq!(items[0]["login"]["uris"][0], "https://gmail.com");
        assert_eq!(items[0]["notes"], "linha 1\nlinha 2");
    }

    #[test]
    fn csv_aspas_escapadas_e_bom() {
        let csv = "\u{feff}name,password\n\"diz \"\"oi\"\"\",segredo\n";
        let items = parse_csv_items(csv);
        assert_eq!(items[0]["name"], "diz \"oi\"");
        assert_eq!(items[0]["login"]["password"], "segredo");
    }

    #[test]
    fn csv_detecta_ponto_e_virgula() {
        let csv = "name;username;password\nBanco;joao;segredo\nOutro;maria;outra\n";
        let items = parse_csv_items(csv);
        assert_eq!(items.len(), 2);
        assert_eq!(items[1]["name"], "Outro");
        assert_eq!(items[1]["login"]["username"], "maria");
        assert_eq!(items[1]["login"]["password"], "outra");
    }

    #[test]
    fn csv_cabecalhos_pt_br_mapeiam_o_que_o_ts_mapeia() {
        // Os COLS do TS têm pt para username/senha/url/nota, mas NÃO para
        // "nome": o name cai pro url (fiel ao original) e "nome" vira campo
        // personalizado — nada se perde.
        let csv = "nome;usuário;senha;endereço;nota\n\
                   Meu banco;joao;segredo;https://b.com;conta principal\n";
        let items = parse_csv_items(csv);
        assert_eq!(items[0]["name"], "https://b.com");
        assert_eq!(items[0]["login"]["username"], "joao");
        assert_eq!(items[0]["login"]["password"], "segredo");
        assert_eq!(items[0]["login"]["uris"][0], "https://b.com");
        assert_eq!(items[0]["notes"], "conta principal");
        let extras = items[0]["customFields"].as_array().unwrap();
        assert!(extras
            .iter()
            .any(|c| c["name"] == "nome" && c["value"] == "Meu banco"));
    }

    #[test]
    fn csv_extras_viram_campos_custom_com_segredo_oculto() {
        let csv = "name,password,pin,project\nGit,segredo,1234,ACME\n";
        let items = parse_csv_items(csv);
        let extras = items[0]["customFields"].as_array().unwrap();
        let pin = extras.iter().find(|c| c["name"] == "pin").unwrap();
        assert_eq!(pin["value"], "1234");
        assert_eq!(pin["hidden"], true, "cabeçalho de segredo fica oculto");
        let project = extras.iter().find(|c| c["name"] == "project").unwrap();
        assert_eq!(project["hidden"], false);
        // id de cada campo custom no formato UUID do oficial
        assert_eq!(project["id"].as_str().unwrap().len(), 36);
    }

    #[test]
    fn csv_email_vira_usuario_e_custom_field_quando_diferente() {
        let csv = "name,email,password\nX,joao@x.com,senha\n";
        let items = parse_csv_items(csv);
        assert_eq!(items[0]["login"]["username"], "joao@x.com");
        // email == username → NÃO duplica em customFields
        assert!(items[0].get("customFields").is_none());
    }

    #[test]
    fn csv_sem_registros_devolve_vazio() {
        assert!(parse_csv_items("").is_empty());
        assert!(parse_csv_items("name,password\n").is_empty());
    }

    #[test]
    fn extract_secret_aceita_otpauth_e_chave_crua() {
        assert_eq!(
            extract_secret("otpauth://totp/Acme:joe?secret=JBSWY3DP&issuer=Acme"),
            "JBSWY3DP"
        );
        assert_eq!(extract_secret("key=ABC123&step=30"), "ABC123");
        assert_eq!(extract_secret("OTPAUTH://totp/x?SECRET=XYZ789&period=30"), "XYZ789");
        assert_eq!(extract_secret("JBSWY3DP"), "JBSWY3DP");
        assert_eq!(extract_secret("  "), "");
    }

    #[test]
    fn bitwarden_importa_login_e_nota_e_conta_card_identity() {
        let json = r#"{"items":[
            {"type":1,"name":"GitHub","notes":"minha conta","favorite":true,
             "login":{"username":"joao","password":"p@ss","totp":"otpauth://totp/x?secret=JBSWY3DP&issuer=x",
                      "uris":[{"uri":"https://github.com"},{"uri":""}]}},
            {"type":2,"name":"Nota segura","notes":"texto"},
            {"type":3,"name":"Cartão"},
            {"type":4,"name":"Identidade"}
        ]}"#;
        let report = parse_bitwarden_json(json).unwrap();
        assert_eq!(report.items.len(), 2);
        assert_eq!(report.skipped_card_identity, 2);

        let login = &report.items[0];
        assert_eq!(login["kind"], "login");
        assert_eq!(login["name"], "GitHub");
        assert_eq!(login["notes"], "minha conta");
        assert_eq!(login["favorite"], true);
        assert_eq!(login["login"]["username"], "joao");
        assert_eq!(login["login"]["password"], "p@ss");
        assert_eq!(login["login"]["totp"], "JBSWY3DP");
        // uri vazia some; sobrou só a real
        assert_eq!(login["login"]["uris"], serde_json::json!(["https://github.com"]));

        let note = &report.items[1];
        assert_eq!(note["kind"], "note");
        assert_eq!(note["notes"], "texto");
        assert!(note.get("login").is_none(), "nota não carrega login");
    }

    #[test]
    fn bitwarden_aceita_array_na_raiz_e_campos_ausentes() {
        let json = r#"[{"type":1,"name":"Sem resto"}]"#;
        let report = parse_bitwarden_json(json).unwrap();
        assert_eq!(report.items.len(), 1);
        // login ausente no export → login vazio no shape do oficial
        assert_eq!(report.items[0]["login"]["username"], "");
        assert_eq!(report.items[0]["login"]["password"], "");
        assert_eq!(report.items[0]["login"]["uris"], serde_json::json!([""]));
        assert_eq!(report.items[0]["login"]["totp"], "");
    }

    #[test]
    fn bitwarden_json_invalido_e_erro() {
        assert!(parse_bitwarden_json("isto não é json").is_err());
    }

    #[test]
    fn kdbx_vira_login_com_fallbacks_de_nome() {
        let entries = vec![
            KdbxEntry {
                name: "Banco".into(),
                username: "joao".into(),
                password: "p".into(),
                url: "https://b.com".into(),
                totp: "JBSWY3DP".into(),
                notes: "n".into(),
            },
            KdbxEntry {
                name: String::new(),
                username: "maria".into(),
                password: "q".into(),
                url: String::new(),
                totp: String::new(),
                notes: String::new(),
            },
        ];
        let items = kdbx_to_items(&entries);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "Banco");
        assert_eq!(items[0]["login"]["uris"][0], "https://b.com");
        assert_eq!(items[0]["login"]["totp"], "JBSWY3DP");
        // sem name/url → username; sem url → uri vazia
        assert_eq!(items[1]["name"], "maria");
        assert_eq!(items[1]["login"]["uris"], serde_json::json!([""]));
    }

    #[test]
    fn detecta_formato_por_extensao_e_conteudo() {
        assert_eq!(detect_format("x.kdbx", None), ImportFormat::Kdbx);
        assert_eq!(detect_format("x.JSON", None), ImportFormat::BitwardenJson);
        assert_eq!(detect_format("x.csv", None), ImportFormat::Csv);
        assert_eq!(
            detect_format("sem-ext", Some("  [{\"a\":1}]")),
            ImportFormat::BitwardenJson
        );
        assert_eq!(detect_format("sem-ext", Some("a,b\n1,2")), ImportFormat::Csv);
        assert_eq!(detect_format("sem-ext", None), ImportFormat::Unknown);
    }
}
