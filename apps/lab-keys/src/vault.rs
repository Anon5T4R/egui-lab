//! Schema do vault `.tkeys` (o JSON dentro do blob cifrado), compatível com o
//! LocalKeys oficial — lá o schema vive no front TS (`src/types.ts`); aqui só
//! o subconjunto que o lab usa.
//!
//! ESCRITA CONSERVADORA: o lab acrescenta itens (login/nota) e anexos a um
//! item. Pastas, campos custom, histórico de senha e o que mais existir num
//! cofre REAL atravessam intactos — manipulamos o JSON bruto
//! (`serde_json::Value`), nunca um struct que re-serializaria só os campos
//! conhecidos.

use serde_json::{json, Value};

/// Vault recém-criado — mesma semente do oficial (`lib.rs` do LocalKeys).
pub const EMPTY_VAULT: &str = r#"{"version":1,"folders":[],"items":[]}"#;

/// Gravação atômica — copiada verbatim do LocalKeys (`lib.rs`): tmp + rename
/// por cima (atómico em Windows e Unix), com `.bak` do estado anterior. Uma
/// gravação interrompida não corrompe o vault.
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        let _ = std::fs::copy(path, format!("{}.bak", path.display()));
    }
    let tmp = format!("{}.tmp", path.display());
    std::fs::write(&tmp, bytes).map_err(|e| format!("falha ao gravar '{tmp}': {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("falha ao substituir '{}': {}", path.display(), e)
    })
}

/// Linha da lista (visão só do que a UI mostra).
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    /// Segredo TOTP (base32) se o item tiver — a UI gera o código ao vivo.
    pub totp: Option<String>,
    pub favorite: bool,
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawItem {
    id: String,
    name: String,
    favorite: bool,
    deleted_at: Option<i64>,
    login: Option<RawLogin>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct RawLogin {
    username: String,
    totp: String,
}

/// Visão de lista do vault (lixeira — `deletedAt` — fica fora, como no oficial).
pub fn items_view(raw: &Value) -> Vec<ItemView> {
    raw.get("items")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    let ri: RawItem = serde_json::from_value(it.clone()).ok()?;
                    if ri.deleted_at.is_some() {
                        return None;
                    }
                    Some(ItemView {
                        id: ri.id,
                        name: ri.name,
                        username: ri.login.as_ref().map(|l| l.username.clone()),
                        totp: ri.login.and_then(|l| {
                            if l.totp.is_empty() {
                                None
                            } else {
                                Some(l.totp)
                            }
                        }),
                        favorite: ri.favorite,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Id no MESMO formato do oficial (`crypto.randomUUID()` do TS): UUID v4
/// com traços (8-4-4-4-12). Era 32-hex sem traços — funcional (id é opaco),
/// mas um parser estrito de UUID reclamaria; agora é indistinguível.
fn gen_id() -> String {
    let mut b = rand::random::<u128>().to_le_bytes();
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
    let h = |x: &[u8]| x.iter().map(|c| format!("{c:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        h(&b[0..4]),
        h(&b[4..6]),
        h(&b[6..8]),
        h(&b[8..10]),
        h(&b[10..16])
    )
}

/// Campos do login de um item novo (só faz sentido para `kind = "login"`).
/// Strings próprias: os importadores montam o TOTP já extraído (dono) e não
/// ganham nada emprestando o buffer da linha do CSV.
pub struct LoginFields {
    pub username: String,
    pub password: String,
    pub uris: Vec<String>,
    pub totp: String,
}

/// Item novo no shape do `emptyItem` do oficial — base comum do `add_login` e
/// dos importadores (`import.rs`). `login` só entra quando o item é de LOGIN:
/// note/card/identity não carregam o campo (mesmo comportamento do TS).
pub struct NewItem<'a> {
    pub kind: &'a str,
    pub name: &'a str,
    pub notes: &'a str,
    pub favorite: bool,
    pub login: Option<LoginFields>,
    pub custom_fields: Vec<Value>,
}

/// Monta o item novo (id/timestamps daqui). Não toca no vault — quem grava é
/// `push_item`/`push_items`.
pub fn make_item(n: NewItem<'_>) -> Value {
    let now = now_ms();
    let mut item = json!({
        "id": gen_id(),
        "kind": n.kind,
        "name": n.name,
        "favorite": n.favorite,
        "folderId": null,
        "notes": n.notes,
        "createdAt": now,
        "updatedAt": now,
        "deletedAt": null,
    });
    if let Some(login) = n.login {
        item["login"] = json!({
            "username": login.username,
            "password": login.password,
            "uris": login.uris,
            "totp": login.totp,
        });
    }
    if !n.custom_fields.is_empty() {
        item["customFields"] = Value::Array(n.custom_fields);
    }
    item
}

/// Campo personalizado no shape do oficial: `{id,name,value,hidden}`.
pub fn custom_field(name: &str, value: &str, hidden: bool) -> Value {
    json!({ "id": gen_id(), "name": name, "value": value, "hidden": hidden })
}

/// Acrescenta um item já montado (`make_item`) ao vault bruto.
pub fn push_item(raw: &mut Value, item: Value) {
    if !raw["items"].is_array() {
        raw["items"] = json!([]);
    }
    raw["items"]
        .as_array_mut()
        .expect("items é array (garantido acima)")
        .push(item);
}

/// Acrescenta um lote de itens — caminho dos importadores.
pub fn push_items(raw: &mut Value, items: Vec<Value>) {
    for item in items {
        push_item(raw, item);
    }
}

/// Campos do form de login — o que a UI coleta pra criar OU editar um item.
/// Strings emprestadas: o chamador é dono dos buffers do formulário.
#[derive(Clone, Copy)]
pub struct LoginForm<'a> {
    pub name: &'a str,
    pub username: &'a str,
    pub password: &'a str,
    pub totp: &'a str,
    /// Primeira URI (o form edita só ela; as demais atravessam intactas).
    pub url: &'a str,
    pub notes: &'a str,
}

/// Tudo que o form de edição mostra de um login (inclui o que não é segredo).
pub struct LoginFull {
    pub username: String,
    pub password: String,
    pub totp: String,
    pub url: String,
    pub notes: String,
}

/// Linha da lixeira: o que a UI mostra e o id pra restaurar/excluir de vez.
pub struct TrashRow {
    pub id: String,
    pub name: String,
    /// `deletedAt` em ms desde o epoch (o oficial grava `Date.now()`).
    pub deleted_at: i64,
}

/// ACRESCENTA um item de login no vault bruto (mesma forma que `emptyItem`
/// do oficial gera pra kind=login — `uris` vazia vira `[""]`).
pub fn add_login(raw: &mut Value, form: LoginForm<'_>) {
    let item = make_item(NewItem {
        kind: "login",
        name: form.name,
        notes: form.notes,
        favorite: false,
        login: Some(LoginFields {
            username: form.username.to_string(),
            password: form.password.to_string(),
            uris: vec![form.url.to_string()],
            totp: form.totp.to_string(),
        }),
        custom_fields: Vec::new(),
    });
    push_item(raw, item);
}

/// `(username, password, totp)` do login pelo id — só leitura.
pub fn login_triple(raw: &Value, id: &str) -> Option<(String, String, String)> {
    raw.get("items")?.as_array()?.iter().find_map(|it| {
        let same = it.get("id").and_then(Value::as_str) == Some(id);
        if !same {
            return None;
        }
        let login = it.get("login")?;
        Some((
            login.get("username").and_then(Value::as_str)?.to_string(),
            login.get("password").and_then(Value::as_str)?.to_string(),
            login
                .get("totp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ))
    })
}

/// Tudo o que o form de edição precisa carregar de um item (inclui `uris[0]`
/// e `notes`). Item de nota/cartão devolve os campos vazios — o form só edita
/// login, e o `edit_login` cria o login que faltava.
pub fn login_full(raw: &Value, id: &str) -> Option<LoginFull> {
    let it = item(raw, id)?;
    let login = it.get("login");
    Some(LoginFull {
        username: login
            .and_then(|l| l.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        password: login
            .and_then(|l| l.get("password"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        totp: login
            .and_then(|l| l.get("totp"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        url: login
            .and_then(|l| l.get("uris"))
            .and_then(|u| u.get(0))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        notes: it
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

/// EDITA um item existente (por id) — mesma escrita conservadora do add: só
/// toca nos campos do próprio item, tudo ao redor atravessa intacto.
///
/// Espelha o `updateItem` do oficial (`store.ts`): se a senha de um LOGIN
/// mudou (e a anterior não era vazia), a anterior vai pro topo do
/// `passwordHistory` com cap de 20 entradas.
///
/// `uris`: só a PRIMEIRA posição entra pelo form — as outras (que o lab não
/// mostra) continuam onde estão; lista vazia vira `[""]`, como o `emptyItem`.
pub fn edit_login(raw: &mut Value, id: &str, form: LoginForm<'_>) -> bool {
    let LoginForm {
        name,
        username,
        password,
        totp,
        url,
        notes,
    } = form;
    let Some(items) = raw.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    for it in items.iter_mut() {
        if it.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        // Histórico: guardar a senha anterior ANTES de sobrescrever
        // (condições idênticas às do oficial: kind login + senha antiga não
        // vazia + senha de fato mudou).
        let old_pass = it
            .get("login")
            .and_then(|l| l.get("password"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let is_login = it.get("kind").and_then(Value::as_str) == Some("login");

        it["name"] = json!(name);
        it["notes"] = json!(notes);
        it["updatedAt"] = json!(now_ms());
        // login pode não existir em item de outra classe → cria com a forma
        // completa (uris vazios, como o emptyItem do oficial).
        if !it.get("login").is_some_and(Value::is_object) {
            it["login"] = json!({ "username": "", "password": "", "uris": [""], "totp": "" });
        }
        if let Some(login) = it.get_mut("login").and_then(Value::as_object_mut) {
            login.insert("username".into(), json!(username));
            login.insert("password".into(), json!(password));
            login.insert("totp".into(), json!(totp));
            // uris: edita a primeira, preserva o resto da lista.
            let mut uris: Vec<Value> = login
                .get("uris")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if uris.is_empty() {
                uris.push(json!(""));
            }
            uris[0] = json!(url);
            login.insert("uris".into(), Value::Array(uris));
        }

        if is_login && !old_pass.is_empty() && old_pass != password {
            let mut hist: Vec<Value> = it
                .get("passwordHistory")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            hist.insert(0, json!({ "password": old_pass, "at": now_ms() }));
            hist.truncate(20); // cap do oficial
            it["passwordHistory"] = Value::Array(hist);
        }
        return true;
    }
    false
}

/// Exclusão LÓGica (lixeira do oficial): marca `deletedAt` — e SÓ ele, como
/// o `trashItem` do oficial (não toca em `updatedAt`). Um `.tkeys` aberto no
/// LocalKeys continua vendo o item na lixeira.
pub fn delete_login(raw: &mut Value, id: &str) -> bool {
    let Some(items) = raw.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    for it in items.iter_mut() {
        if it.get("id").and_then(Value::as_str) == Some(id) {
            it["deletedAt"] = json!(now_ms());
            return true;
        }
    }
    false
}

/// Favorita/desfavorita — escrita conservadora: só `favorite` do item alvo.
///
/// Espelha o oficial: o ★ do editor chama `updateItem`, que grava
/// `updatedAt: Date.now()` junto (o store não tem um `toggleFavorite` à
/// parte) — então favoritar MEXE em `updatedAt`, e o lab faz igual.
pub fn set_favorite(raw: &mut Value, id: &str, favorite: bool) -> bool {
    let Some(it) = item_mut(raw, id) else {
        return false;
    };
    it["favorite"] = json!(favorite);
    it["updatedAt"] = json!(now_ms());
    true
}

/// Restaura da lixeira — igual ao `restoreItem` do oficial: zera `deletedAt`
/// e não toca em mais nada (nem `updatedAt`).
pub fn restore_item(raw: &mut Value, id: &str) -> bool {
    let Some(it) = item_mut(raw, id) else {
        return false;
    };
    it["deletedAt"] = Value::Null;
    true
}

/// Exclui DE VEZ (fora da lixeira) — igual ao `deleteForever` do oficial:
/// tira o item do array, sem deixar rastro.
pub fn delete_forever(raw: &mut Value, id: &str) -> bool {
    let Some(items) = raw.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    let before = items.len();
    items.retain(|it| it.get("id").and_then(Value::as_str) != Some(id));
    items.len() != before
}

/// Itens na lixeira, na ordem do array (como o filtro "trash" do oficial).
pub fn trashed_items(raw: &Value) -> Vec<TrashRow> {
    raw.get("items")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|it| {
                    // 0 é falsy no JS (`if (i.deletedAt)`) — mesma regra.
                    let deleted_at = it.get("deletedAt")?.as_i64()?;
                    if deleted_at == 0 {
                        return None;
                    }
                    Some(TrashRow {
                        id: it.get("id")?.as_str()?.to_string(),
                        name: it
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        deleted_at,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn item_mut<'a>(raw: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    raw.get_mut("items")?
        .as_array_mut()?
        .iter_mut()
        .find(|it| it.get("id").and_then(Value::as_str) == Some(id))
}

fn item<'a>(raw: &'a Value, id: &str) -> Option<&'a Value> {
    raw.get("items")?
        .as_array()?
        .iter()
        .find(|it| it.get("id").and_then(Value::as_str) == Some(id))
}

/// Anexa um arquivo a um item — escrita conservadora: só mexe no array
/// `attachments` do item alvo, o resto do cofre atravessa intacto.
///
/// `data_b64` é o conteúdo já em base64 (o blob `.tkeys` cifra o item inteiro,
/// então o anexo não precisa de cifra própria). `size` é o tamanho dos bytes
/// DECODIFICADOS, como o oficial espera. Base64 inválido ou item inexistente →
/// `false` e nada muda.
pub fn add_attachment(
    raw: &mut Value,
    item_id: &str,
    name: &str,
    mime: &str,
    data_b64: &str,
) -> bool {
    use base64::Engine as _;

    let data_b64 = data_b64.trim();
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data_b64) else {
        return false;
    };
    let Some(it) = item_mut(raw, item_id) else {
        return false;
    };
    let att = json!({
        "id": gen_id(),
        "name": name,
        "size": bytes.len(),
        "mime": mime,
        "dataB64": data_b64,
    });
    if !it.get("attachments").is_some_and(Value::is_array) {
        it["attachments"] = json!([]);
    }
    it["attachments"]
        .as_array_mut()
        .expect("attachments é array (garantido acima)")
        .push(att);
    true
}

/// `(name, dataB64)` de um anexo — o que a UI precisa pra salvar em disco.
pub fn attachment_data(
    raw: &Value,
    item_id: &str,
    attachment_id: &str,
) -> Option<(String, String)> {
    let atts = item(raw, item_id)?.get("attachments")?.as_array()?;
    let att = atts
        .iter()
        .find(|a| a.get("id").and_then(Value::as_str) == Some(attachment_id))?;
    Some((
        att.get("name").and_then(Value::as_str)?.to_string(),
        att.get("dataB64").and_then(Value::as_str)?.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Form mínimo pra maioria dos testes (sem url/notas/totp).
    fn form<'a>(name: &'a str, username: &'a str, password: &'a str) -> LoginForm<'a> {
        LoginForm {
            name,
            username,
            password,
            totp: "",
            url: "",
            notes: "",
        }
    }

    fn vault_com_item() -> Value {
        let mut v: Value = serde_json::from_str(EMPTY_VAULT).unwrap();
        add_login(&mut v, form("Gmail", "joao", "secreta"));
        v
    }

    #[test]
    fn add_login_popula_items() {
        let v = vault_com_item();
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "login");
        assert_eq!(items[0]["name"], "Gmail");
        assert_eq!(items[0]["login"]["username"], "joao");
        // campos camelCase exatamente como o oficial espera
        assert!(items[0].get("createdAt").is_some());
        assert!(items[0].get("deletedAt").is_some());
        assert!(items[0].get("folderId").is_some());
    }

    #[test]
    fn view_esconde_lixeira() {
        let mut v = vault_com_item();
        v["items"][0]["deletedAt"] = json!(1700000000000i64);
        assert!(items_view(&v).is_empty());
    }

    #[test]
    fn login_triple_devolve_as_credenciais() {
        let v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert_eq!(
            login_triple(&v, &id),
            Some(("joao".into(), "secreta".into(), "".into()))
        );
        assert_eq!(login_triple(&v, "id-que-nao-existe"), None);
    }

    #[test]
    fn add_preserva_campos_desconhecidos() {
        // Um cofre REAL tem anexos/campos custom que o lab não conhece —
        // acrescentar item não pode derrubar nada disso.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[{"id":"f1","name":"Pessoal"}],
               "items":[{"id":"x","kind":"note","name":"nota",
                          "customFields":[{"id":"c1","name":"k","value":"v","hidden":false},
                                          {"id":"c2","name":"k2","value":"v2","hidden":true}],
                          "attachments":[{"id":"a1","name":"f.png","size":10,"mime":"image/png","dataB64":"aGk="}]}]}"#,
        )
        .unwrap();
        add_login(&mut v, form("Novo", "u", "p"));
        assert_eq!(v["folders"][0]["name"], "Pessoal");
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["customFields"].as_array().unwrap().len(), 2);
        assert_eq!(v["items"][0]["attachments"][0]["mime"], "image/png");
        assert_eq!(v["items"][1]["login"]["password"], "p");
    }

    #[test]
    fn view_mostra_totp_quando_existe() {
        let mut v = vault_com_item();
        v["items"][0]["login"]["totp"] = json!("JBSWY3DPEHPK3PXP");
        let items = items_view(&v);
        assert_eq!(items[0].totp.as_deref(), Some("JBSWY3DPEHPK3PXP"));

        let mut v2 = vault_com_item(); // sem totp
        v2["items"][0]["login"]["totp"] = json!("");
        assert!(items_view(&v2)[0].totp.is_none());
    }

    #[test]
    fn edita_login_sem_derrubar_o_resto() {
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[{"id":"f1","name":"Pessoal"}],
               "items":[{"id":"x","kind":"login","name":"Antigo","favorite":true,"folderId":"f1",
                          "notes":"keep me","createdAt":1,"updatedAt":1,"deletedAt":null,
                          "login":{"username":"u","password":"p","uris":["https://a"],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(
            &mut v,
            "x",
            LoginForm {
                name: "Novo",
                username: "user2",
                password: "pass2",
                totp: "JBSWY3DPEHPK3PXP",
                // o form devolve a URL que carregou — a lista não muda
                url: "https://a",
                notes: "keep me",
            }
        ));
        let it = &v["items"][0];
        assert_eq!(it["name"], "Novo");
        assert_eq!(it["login"]["username"], "user2");
        assert_eq!(it["login"]["password"], "pass2");
        assert_eq!(it["login"]["totp"], "JBSWY3DPEHPK3PXP");
        // o que NÃO foi editado atravessa intacto
        assert_eq!(it["favorite"], true);
        assert_eq!(it["folderId"], "f1");
        assert_eq!(it["notes"], "keep me");
        assert_eq!(it["login"]["uris"][0], "https://a");
        assert_eq!(v["folders"][0]["name"], "Pessoal");
        // id inexistente: false e nada muda (compara o item antes/depois —
        // sem segurar borrow imutável através do &mut)
        let antes = serde_json::to_string(&v["items"][0]).unwrap();
        assert!(!edit_login(&mut v, "não-existe", form("n", "u", "p")));
        let depois = serde_json::to_string(&v["items"][0]).unwrap();
        assert_eq!(antes, depois);
    }

    #[test]
    fn editar_url_troca_a_primeira_uri_e_preserva_as_outras() {
        // O form edita só uris[0]; o resto da lista (que o lab não mostra)
        // continua onde estava — escrita conservadora.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"N",
               "favorite":false,"notes":"","login":{"username":"u","password":"p",
               "uris":["https://velho","https://segundo","https://terceiro"],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(
            &mut v,
            "x",
            LoginForm {
                url: "https://novo",
                ..form("N", "u", "p")
            }
        ));
        assert_eq!(
            v["items"][0]["login"]["uris"],
            json!(["https://novo", "https://segundo", "https://terceiro"])
        );
    }

    #[test]
    fn editar_sem_url_deixa_a_lista_como_o_empty_item() {
        // uris ausente/vazia → [""], igual ao emptyItem do oficial.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"N",
               "favorite":false,"notes":"","login":{"username":"u","password":"p","uris":[],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(&mut v, "x", form("N", "u", "p")));
        assert_eq!(v["items"][0]["login"]["uris"], json!([""]));
    }

    #[test]
    fn editar_grava_as_notas() {
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"Gmail",
               "favorite":false,"notes":"velho","updatedAt":1,"deletedAt":null,
               "passwordHistory":[{"password":"mais-velha","at":1}],
               "login":{"username":"joao","password":"secreta","uris":["https://a"],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(
            &mut v,
            "x",
            LoginForm {
                notes: "recado novo\nsegunda linha",
                ..form("Gmail", "joao", "secreta")
            }
        ));
        assert_eq!(v["items"][0]["notes"], "recado novo\nsegunda linha");
        // editar nota/url não mexe no histórico (senha não mudou)
        assert_eq!(
            v["items"][0]["passwordHistory"],
            json!([{ "password": "mais-velha", "at": 1 }])
        );
    }

    #[test]
    fn add_login_com_url_e_notas() {
        let mut v: Value = serde_json::from_str(EMPTY_VAULT).unwrap();
        add_login(
            &mut v,
            LoginForm {
                name: "GitHub",
                username: "joao",
                password: "p",
                totp: "JBSWY3DPEHPK3PXP",
                url: "https://github.com",
                notes: "conta pessoal",
            },
        );
        let it = &v["items"][0];
        assert_eq!(it["login"]["uris"], json!(["https://github.com"]));
        assert_eq!(it["login"]["totp"], "JBSWY3DPEHPK3PXP");
        assert_eq!(it["notes"], "conta pessoal");
    }

    #[test]
    fn login_full_carrega_o_que_o_form_mostra() {
        let v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"N",
               "notes":"nota","login":{"username":"u","password":"p",
               "uris":["https://a","https://b"],"totp":"JBSWY3DPEHPK3PXP"}}]}"#,
        )
        .unwrap();
        let full = login_full(&v, "x").expect("item existe");
        assert_eq!(full.username, "u");
        assert_eq!(full.password, "p");
        assert_eq!(full.totp, "JBSWY3DPEHPK3PXP");
        assert_eq!(full.url, "https://a", "só a primeira URI");
        assert_eq!(full.notes, "nota");
        assert!(login_full(&v, "não-existe").is_none());
    }

    #[test]
    fn favoritar_mexe_no_updated_at_como_o_update_item_do_oficial() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        v["items"][0]["updatedAt"] = json!(1);
        assert!(set_favorite(&mut v, &id, true));
        assert_eq!(v["items"][0]["favorite"], true);
        assert_ne!(
            v["items"][0]["updatedAt"],
            json!(1),
            "updateItem grava updatedAt"
        );
        assert!(set_favorite(&mut v, &id, false));
        assert_eq!(v["items"][0]["favorite"], false);
        assert!(!set_favorite(&mut v, "não-existe", true));
    }

    #[test]
    fn restaurar_zera_o_deleted_at_sem_mexer_no_resto() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert!(delete_login(&mut v, &id));
        let updated_antes = v["items"][0]["updatedAt"].clone();
        assert!(restore_item(&mut v, &id));
        assert_eq!(v["items"][0]["deletedAt"], Value::Null);
        assert_eq!(
            v["items"][0]["updatedAt"], updated_antes,
            "restoreItem não toca updatedAt"
        );
        assert_eq!(items_view(&v).len(), 1, "volta pra lista");
        assert!(!restore_item(&mut v, "não-existe"));
    }

    #[test]
    fn excluir_de_vez_tira_do_array() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert!(delete_forever(&mut v, &id));
        assert!(v["items"].as_array().unwrap().is_empty());
        assert!(!delete_forever(&mut v, &id));
        assert_eq!(v["folders"], json!([]), "só o item sai");
    }

    #[test]
    fn lixeira_lista_so_os_deletados_com_a_data() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert!(trashed_items(&v).is_empty());
        v["items"][0]["deletedAt"] = json!(1_700_000_000_000i64);
        let rows = trashed_items(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].name, "Gmail");
        assert_eq!(rows[0].deleted_at, 1_700_000_000_000);
        // deletedAt null (o oficial grava null ao restaurar) = fora da lixeira
        v["items"][0]["deletedAt"] = Value::Null;
        assert!(trashed_items(&v).is_empty());
    }

    #[test]
    fn editar_item_sem_login_cria_o_login() {
        // note/card não têm login; se o usuário "editar" um desses no lab,
        // o login nasce com a forma completa do oficial.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"n1","kind":"note","name":"nota",
               "favorite":false,"folderId":null,"notes":"","createdAt":1,"updatedAt":1,"deletedAt":null}]}"#,
        )
        .unwrap();
        assert!(edit_login(&mut v, "n1", form("nota", "u", "p")));
        assert_eq!(v["items"][0]["login"]["username"], "u");
        assert_eq!(v["items"][0]["login"]["uris"], json!([""]));
        assert_eq!(v["items"][0]["kind"], "note"); // classe intacta
    }

    #[test]
    fn exclusao_e_logica_e_some_da_view() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        // updatedAt original, pra provar que a lixeira NÃO o mexe (o
        // trashItem do oficial só toca em deletedAt).
        let updated_antes = v["items"][0]["updatedAt"].clone();
        assert!(delete_login(&mut v, &id));
        // linha continua no JSON (lixeira do oficial)...
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert!(v["items"][0]["deletedAt"].is_number());
        assert_eq!(v["items"][0]["updatedAt"], updated_antes);
        // ...mas fora da lista
        assert!(items_view(&v).is_empty());
        assert!(!delete_login(&mut v, "não-existe"));
    }

    #[test]
    fn id_igual_ao_randomuuid_do_oficial() {
        // 8-4-4-4-12, nibble de versão 4 e variante 8/9/a/b — o formato
        // exato de crypto.randomUUID() no TS.
        let id = gen_id();
        let part: Vec<&str> = id.split('-').collect();
        assert_eq!(
            part.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(&id[14..15], "4", "version nibble");
        assert!(
            ['8', '9', 'a', 'b'].contains(&id.chars().nth(19).unwrap()),
            "variante RFC 4122"
        );
        assert_ne!(gen_id(), gen_id());
    }

    #[test]
    fn editar_senha_guarda_a_anterior_no_historico() {
        // Mesma regra do updateItem do oficial: login + senha antiga não vazia
        // + senha mudou → {password, at} no topo, cap 20.
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"N",
               "favorite":false,"folderId":null,"notes":"","createdAt":1,"updatedAt":1,"deletedAt":null,
               "login":{"username":"u","password":"velha","uris":[],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(&mut v, "x", form("N", "u", "nova1")));
        let hist = v["items"][0]["passwordHistory"].as_array().unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0]["password"], "velha");
        assert!(hist[0]["at"].is_number());

        // Segunda troca: a "nova1" entra no TOPO, a "velha" desce.
        assert!(edit_login(&mut v, "x", form("N", "u", "nova2")));
        let hist = v["items"][0]["passwordHistory"].as_array().unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0]["password"], "nova1");
        assert_eq!(hist[1]["password"], "velha");
    }

    #[test]
    fn editar_sem_trocar_senha_nao_cria_historico() {
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[],"items":[{"id":"x","kind":"login","name":"N",
               "favorite":false,"folderId":null,"notes":"","createdAt":1,"updatedAt":1,"deletedAt":null,
               "login":{"username":"u","password":"igual","uris":[],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(edit_login(&mut v, "x", form("Outro nome", "u2", "igual")));
        assert!(
            v["items"][0].get("passwordHistory").is_none(),
            "senha não mudou → sem histórico (igual ao oficial)"
        );
        assert_eq!(v["items"][0]["name"], "Outro nome");
    }

    #[test]
    fn item_de_nota_nao_carrega_login() {
        // O emptyItem("note") do oficial não tem `login` — os importadores
        // dependem disso pra nota não virar login sem senha.
        let item = make_item(NewItem {
            kind: "note",
            name: "Nota",
            notes: "texto",
            favorite: true,
            login: None,
            custom_fields: Vec::new(),
        });
        assert_eq!(item["kind"], "note");
        assert_eq!(item["notes"], "texto");
        assert_eq!(item["favorite"], true);
        assert!(item.get("login").is_none());
        assert!(item.get("customFields").is_none());
    }

    #[test]
    fn anexo_entra_com_tamanho_decodificado() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        // "hi" em base64 → 2 bytes decodificados (o size é do conteúdo, não do b64)
        assert!(add_attachment(&mut v, &id, "f.txt", "text/plain", "aGk="));
        let atts = v["items"][0]["attachments"].as_array().unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0]["name"], "f.txt");
        assert_eq!(atts[0]["mime"], "text/plain");
        assert_eq!(atts[0]["size"], 2);
        assert_eq!(atts[0]["dataB64"], "aGk=");
        // id no formato UUID do oficial
        let att_id = atts[0]["id"].as_str().unwrap();
        assert_eq!(att_id.len(), 36);
        assert_eq!(
            attachment_data(&v, &id, att_id),
            Some(("f.txt".into(), "aGk=".into()))
        );
    }

    #[test]
    fn anexo_preserva_o_resto_do_cofre() {
        let mut v: Value = serde_json::from_str(
            r#"{"version":1,"folders":[{"id":"f1","name":"Pessoal"}],
               "items":[{"id":"x","kind":"login","name":"N","favorite":true,"folderId":"f1",
                          "notes":"keep","customFields":[{"id":"c1","name":"k","value":"v","hidden":false}],
                          "attachments":[{"id":"a1","name":"velho.png","size":3,"mime":"image/png","dataB64":"YWJj"}],
                          "login":{"username":"u","password":"p","uris":["https://a"],"totp":""}}]}"#,
        )
        .unwrap();
        assert!(add_attachment(
            &mut v,
            "x",
            "novo.txt",
            "text/plain",
            "aGk="
        ));
        assert_eq!(v["folders"][0]["name"], "Pessoal");
        assert_eq!(v["items"][0]["notes"], "keep");
        assert_eq!(v["items"][0]["customFields"][0]["value"], "v");
        let atts = v["items"][0]["attachments"].as_array().unwrap();
        assert_eq!(atts.len(), 2, "anexo existente não pode ser derrubado");
        assert_eq!(atts[0]["name"], "velho.png");
        assert_eq!(atts[1]["name"], "novo.txt");
    }

    #[test]
    fn anexo_invalido_nao_grava_nada() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        let antes = serde_json::to_string(&v).unwrap();
        assert!(!add_attachment(
            &mut v,
            &id,
            "f",
            "text/plain",
            "não-é-base64!!"
        ));
        assert!(!add_attachment(
            &mut v,
            "id-que-nao-existe",
            "f",
            "text/plain",
            "aGk="
        ));
        assert_eq!(serde_json::to_string(&v).unwrap(), antes);
        assert_eq!(attachment_data(&v, &id, "a-nao-existe"), None);
        assert_eq!(attachment_data(&v, "id-que-nao-existe", "a1"), None);
    }

    #[test]
    fn push_items_acrescenta_o_lote() {
        let mut v = vault_com_item();
        let lote = vec![
            make_item(NewItem {
                kind: "login",
                name: "A",
                notes: "",
                favorite: false,
                login: Some(LoginFields {
                    username: "u".into(),
                    password: "p".into(),
                    uris: vec!["https://a".into()],
                    totp: String::new(),
                }),
                custom_fields: vec![custom_field("pin", "1234", true)],
            }),
            make_item(NewItem {
                kind: "note",
                name: "B",
                notes: "",
                favorite: false,
                login: None,
                custom_fields: Vec::new(),
            }),
        ];
        push_items(&mut v, lote);
        assert_eq!(v["items"].as_array().unwrap().len(), 3);
        assert_eq!(v["items"][1]["customFields"][0]["name"], "pin");
        assert_eq!(v["items"][1]["customFields"][0]["hidden"], true);
        assert_eq!(v["items"][2]["kind"], "note");
    }

    #[test]
    fn atomic_write_escreve_e_reescreve() {
        let dir = std::env::temp_dir().join(format!(
            "lab-keys-aw-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("v.tkeys");
        atomic_write(&p, b"um").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"um");
        atomic_write(&p, b"dois").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"dois");
        assert_eq!(std::fs::read(dir.join("v.tkeys.bak")).unwrap(), b"um");
        std::fs::remove_dir_all(&dir).ok();
    }
}
