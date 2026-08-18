//! Schema do vault `.tkeys` (o JSON dentro do blob cifrado), compatível com o
//! LocalKeys oficial — lá o schema vive no front TS (`src/types.ts`); aqui só
//! o subconjunto que o lab usa.
//!
//! ESCRITA CONSERVADORA: o lab só ACRESCENTA itens de login. Pastas, anexos,
//! campos custom, histórico de senha e itens de outros tipos de um cofre REAL
//! atravessam intactos — manipulamos o JSON bruto (`serde_json::Value`), nunca
//! um struct que re-serializaria só os campos conhecidos.

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
                        totp: ri
                            .login
                            .and_then(|l| if l.totp.is_empty() { None } else { Some(l.totp) }),
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

/// Id opaco único (o oficial usa uuid v4 do TS; aqui 128 bits aleatórios em
/// hex — mesma forma, mesmo tamanho, mesmo significado de "string única").
fn gen_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// ACRESCENTA um item de login no vault bruto (mesma forma que `emptyItem`
/// do oficial gera pra kind=login).
pub fn add_login(raw: &mut Value, name: &str, username: &str, password: &str) {
    let item = json!({
        "id": gen_id(),
        "kind": "login",
        "name": name,
        "favorite": false,
        "folderId": null,
        "notes": "",
        "createdAt": now_ms(),
        "updatedAt": now_ms(),
        "deletedAt": null,
        "login": { "username": username, "password": password, "uris": [""], "totp": "" }
    });
    if !raw["items"].is_array() {
        raw["items"] = json!([]);
    }
    raw["items"]
        .as_array_mut()
        .expect("items é array (garantido acima)")
        .push(item);
}

/// `(username, password, totp)` do login pelo id — só leitura.
pub fn login_triple(raw: &Value, id: &str) -> Option<(String, String, String)> {
    raw.get("items")?
        .as_array()?
        .iter()
        .find_map(|it| {
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

/// EDITA um item existente (por id) — mesma escrita conservadora do add: só
/// toca nos campos do próprio item, tudo ao redor atravessa intacto.
pub fn edit_login(
    raw: &mut Value,
    id: &str,
    name: &str,
    username: &str,
    password: &str,
    totp: &str,
) -> bool {
    let Some(items) = raw.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    for it in items.iter_mut() {
        if it.get("id").and_then(Value::as_str) != Some(id) {
            continue;
        }
        it["name"] = json!(name);
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
        }
        return true;
    }
    false
}

/// Exclusão LÓGICA (lixeira do oficial): marca `deletedAt`, não remove — um
/// `.tkeys` aberto no LocalKeys continua vendo o item na lixeira.
pub fn delete_login(raw: &mut Value, id: &str) -> bool {
    let Some(items) = raw.get_mut("items").and_then(Value::as_array_mut) else {
        return false;
    };
    for it in items.iter_mut() {
        if it.get("id").and_then(Value::as_str) == Some(id) {
            it["deletedAt"] = json!(now_ms());
            it["updatedAt"] = json!(now_ms());
            return true;
        }
    }
    false
}

/// `(username, password)` do login pelo id — só leitura, para o botão copiar.
pub fn login_pair(raw: &Value, id: &str) -> Option<(String, String)> {
    login_triple(raw, id).map(|(u, p, _)| (u, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_com_item() -> Value {
        let mut v: Value = serde_json::from_str(EMPTY_VAULT).unwrap();
        add_login(&mut v, "Gmail", "joao", "secreta");
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
    fn login_pair_devolve_as_credenciais() {
        let v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert_eq!(
            login_pair(&v, &id),
            Some(("joao".into(), "secreta".into()))
        );
        assert_eq!(login_pair(&v, "id-que-nao-existe"), None);
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
        add_login(&mut v, "Novo", "u", "p");
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
        assert!(edit_login(&mut v, "x", "Novo", "user2", "pass2", "JBSWY3DPEHPK3PXP"));
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
        assert!(!edit_login(&mut v, "não-existe", "n", "u", "p", ""));
        let depois = serde_json::to_string(&v["items"][0]).unwrap();
        assert_eq!(antes, depois);
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
        assert!(edit_login(&mut v, "n1", "nota", "u", "p", ""));
        assert_eq!(v["items"][0]["login"]["username"], "u");
        assert_eq!(v["items"][0]["login"]["uris"], json!([""]));
        assert_eq!(v["items"][0]["kind"], "note"); // classe intacta
    }

    #[test]
    fn exclusao_e_logica_e_some_da_view() {
        let mut v = vault_com_item();
        let id = v["items"][0]["id"].as_str().unwrap().to_string();
        assert!(delete_login(&mut v, &id));
        // linha continua no JSON (lixeira do oficial)...
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert!(v["items"][0]["deletedAt"].is_number());
        // ...mas fora da lista
        assert!(items_view(&v).is_empty());
        assert!(!delete_login(&mut v, "não-existe"));
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
