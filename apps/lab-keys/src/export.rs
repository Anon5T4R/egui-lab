//! Export do cofre em CLARO (JSON ou CSV) — porte fiel de
//! `LocalKeys/src/export.ts`. SEM cifra de propósito: serve só pra migrar pra
//! outro app, e a UI mostra um aviso forte antes de gravar.
//!
//! Diferença de FORMA (não de conteúdo): o `serde_json` do lab (sem a feature
//! `preserve_order`) ordena as chaves alfabeticamente, então o JSON sai com as
//! chaves em outra ordem que o `JSON.stringify` do TS. Os dados são os mesmos.

use serde_json::Value;

/// JSON do cofre com os itens VIVOS (lixeira fora), identado com 2 espaços —
/// mesmo recorte do `exportJson` do oficial (`{...vault, items: só vivos}`):
/// pastas e o que mais existir na raiz atravessam intactos.
pub fn export_json(raw: &Value) -> String {
    let live: Vec<Value> = items(raw)
        .iter()
        .filter(|it| !is_trashed(it))
        .cloned()
        .collect();
    let mut out = raw.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("items".to_string(), Value::Array(live));
    }
    // `Value` sempre serializa; o fallback só existe pra não ter `unwrap`.
    serde_json::to_string_pretty(&out).unwrap_or_default()
}

/// CSV do cofre, cabeçalho fixo e linhas terminadas em CRLF — igual ao
/// `exportCsv` do oficial (mesma ordem de colunas e mesmos fallbacks de
/// `card` quando o item não tem `login`).
pub fn export_csv(raw: &Value) -> String {
    let headers = [
        "name", "type", "username", "password", "url", "totp", "notes", "favorite",
    ];
    let mut out = headers.join(",");
    for it in items(raw) {
        if is_trashed(it) {
            continue;
        }
        let login = it.get("login");
        let card = it.get("card");
        let cells = [
            pick(&[it.get("name")]),
            pick(&[it.get("kind")]),
            // `it.login?.username ?? it.card?.cardholder ?? ""`
            pick(&[
                login.and_then(|l| l.get("username")),
                card.and_then(|c| c.get("cardholder")),
            ]),
            // `it.login?.password ?? it.card?.number ?? it.card?.code ?? ""`
            pick(&[
                login.and_then(|l| l.get("password")),
                card.and_then(|c| c.get("number")),
                card.and_then(|c| c.get("code")),
            ]),
            // `it.login?.uris?.[0] ?? ""`
            pick(&[login.and_then(|l| l.get("uris")).and_then(|u| u.get(0))]),
            pick(&[login.and_then(|l| l.get("totp"))]),
            pick(&[it.get("notes")]),
            if it.get("favorite").and_then(Value::as_bool) == Some(true) {
                "1"
            } else {
                "0"
            }
            .to_string(),
        ];
        out.push_str("\r\n");
        out.push_str(&cells.map(|cell| csv_escape(&cell)).join(","));
    }
    out
}

/// Escapa uma célula como o `csvEscape` do TS: só quando há aspa, vírgula ou
/// quebra de linha; a aspa dobra (`"` → `""`).
fn csv_escape(value: &str) -> String {
    let needs_quotes = value.chars().any(|c| matches!(c, '"' | ',' | '\n' | '\r'));
    if needs_quotes {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// `a ?? b ?? ""` do TS: a primeira string que EXISTE vence — string vazia
/// conta como valor (o oficial não cai pro próximo campo por causa dela).
fn pick(candidates: &[Option<&Value>]) -> String {
    candidates
        .iter()
        .flatten()
        .find_map(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn items(raw: &Value) -> &[Value] {
    raw.get("items")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

/// `if (it.deletedAt) continue;` do TS: ausente/null = vivo; qualquer
/// timestamp = lixeira. (O 0 do epoch é falsy no JS — mesmo efeito aqui.)
fn is_trashed(item: &Value) -> bool {
    match item.get("deletedAt") {
        Some(v) => !v.is_null() && v.as_i64() != Some(0),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vault() -> Value {
        json!({
            "version": 1,
            "folders": [{ "id": "f1", "name": "Pessoal" }],
            "items": [
                {
                    "id": "1", "kind": "login", "name": "Gmail", "favorite": true,
                    "notes": "linha1\nlinha2", "deletedAt": null,
                    "login": { "username": "joao", "password": "p,ss\"o",
                               "uris": ["https://mail.google.com", "https://outro"],
                               "totp": "JBSWY3DPEHPK3PXP" }
                },
                {
                    "id": "2", "kind": "note", "name": "Nota", "favorite": false,
                    "notes": "texto", "deletedAt": null
                },
                {
                    "id": "3", "kind": "login", "name": "Na lixeira",
                    "favorite": false, "deletedAt": 1_700_000_000_000i64,
                    "login": { "username": "x", "password": "y", "uris": [], "totp": "" }
                }
            ]
        })
    }

    #[test]
    fn json_leva_so_os_itens_vivos_e_preserva_a_raiz() {
        let out = export_json(&vault());
        let v: Value = serde_json::from_str(&out).expect("json válido");
        let items = v["items"].as_array().expect("items");
        assert_eq!(items.len(), 2, "lixeira fica fora do export");
        assert_eq!(items[0]["name"], "Gmail");
        assert_eq!(v["folders"][0]["name"], "Pessoal");
        assert_eq!(v["version"], 1);
        // pretty com 2 espaços (mesmo `JSON.stringify(v, null, 2)`)
        assert!(out.contains("\n  \"items\""), "{out}");
        assert!(out.contains("\n      \"name\": \"Gmail\""), "{out}");
    }

    #[test]
    fn json_de_cofre_vazio_nao_explode() {
        let v: Value =
            serde_json::from_str(r#"{"version":1,"folders":[],"items":[]}"#).expect("semente");
        let out = export_json(&v);
        assert!(out.contains("\"items\": []"), "{out}");
    }

    #[test]
    fn csv_tem_cabecalho_crlf_e_pula_a_lixeira() {
        let out = export_csv(&vault());
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "name,type,username,password,url,totp,notes,favorite"
        );
        assert_eq!(lines.len(), 3, "cabeçalho + 2 vivos: {out:?}");
        assert!(!out.contains("Na lixeira"));
        assert_eq!(
            lines[1],
            "Gmail,login,joao,\"p,ss\"\"o\",https://mail.google.com,JBSWY3DPEHPK3PXP,\"linha1\nlinha2\",1"
        );
        assert_eq!(lines[2], "Nota,note,,,,,texto,0");
    }

    #[test]
    fn csv_usa_o_card_quando_nao_ha_login() {
        let v = json!({ "items": [{
            "id": "c", "kind": "card", "name": "Visa", "favorite": false,
            "card": { "cardholder": "JOAO", "number": "4111", "code": "123" }
        }]});
        let out = export_csv(&v);
        let row = out.split("\r\n").nth(1).unwrap_or_default();
        assert_eq!(row, "Visa,card,JOAO,4111,,,,0");
    }

    #[test]
    fn csv_prefere_a_senha_do_login_ao_numero_do_cartao() {
        // `login.password ?? card.number`: existindo login, o cartão não entra.
        let v = json!({ "items": [{
            "id": "x", "kind": "login", "name": "N", "favorite": false,
            "login": { "username": "u", "password": "", "uris": [""], "totp": "" },
            "card": { "number": "4111" }
        }]});
        let out = export_csv(&v);
        assert_eq!(
            out.split("\r\n").nth(1).unwrap_or_default(),
            "N,login,u,,,,,0"
        );
    }

    #[test]
    fn csv_escape_so_quando_precisa() {
        assert_eq!(csv_escape("simples"), "simples");
        assert_eq!(csv_escape("com,vírgula"), "\"com,vírgula\"");
        assert_eq!(csv_escape("com\"aspa"), "\"com\"\"aspa\"");
        assert_eq!(csv_escape("linha\nquebrada"), "\"linha\nquebrada\"");
        assert_eq!(csv_escape("cr\r"), "\"cr\r\"");
    }
}
