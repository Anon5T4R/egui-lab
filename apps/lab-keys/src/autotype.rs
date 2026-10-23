//! Auto-type: digita usuário/senha direto no campo focado de OUTRO app, sem
//! passar pelo clipboard (que pode ter histórico/nuvem).
//!
//! Só no Windows: no Linux o `enigo` puxaria X11 dev e engordaria o AppImage
//! (mesma política do keyring/rfd). Fora do Windows o stub devolve erro claro
//! e a UI esconde a opção.

#[cfg(windows)]
pub fn type_login(user: &str, password: &str) -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("auto-type indisponível: {e}"))?;
    // A COMPOSIÇÃO fica aqui (usuário → Tab → senha): quem chama entrega só as
    // credenciais. Tab é o fluxo clássico de formulário login → senha; o
    // oficial digita um texto só e deixa a ordem por conta do chamador.
    enigo
        .text(user)
        .map_err(|e| format!("falha ao digitar o usuário: {e}"))?;
    enigo
        .key(Key::Tab, Direction::Click)
        .map_err(|e| format!("falha ao pressionar Tab: {e}"))?;
    enigo
        .text(password)
        .map_err(|e| format!("falha ao digitar a senha: {e}"))
}

#[cfg(not(windows))]
#[allow(dead_code)] // no Linux o botão de auto-type nem é desenhado (stub sem chamador)
pub fn type_login(_user: &str, _password: &str) -> Result<(), String> {
    Err("auto-type só no Windows".into())
}
