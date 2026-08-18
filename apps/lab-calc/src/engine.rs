//! Motor de expressão do lab-calc — tokenizer + shunting-yard + RPN, o mesmo
//! desenho do motor TS do LocalCalc oficial (lá são ~40 testes; aqui o núcleo
//! do modo padrão: + - * / % ^, parênteses, menos unário, vírgula decimal PT).

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tok {
    Num(f64),
    Neg,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    L,
    R,
}

fn is_op(t: Tok) -> bool {
    matches!(t, Tok::Neg | Tok::Add | Tok::Sub | Tok::Mul | Tok::Div | Tok::Mod | Tok::Pow)
}

fn prec(t: Tok) -> i32 {
    match t {
        Tok::Add | Tok::Sub => 1,
        Tok::Mul | Tok::Div | Tok::Mod => 2,
        Tok::Neg => 3,
        Tok::Pow => 4,
        _ => 0,
    }
}

fn right_assoc(t: Tok) -> bool {
    matches!(t, Tok::Neg | Tok::Pow)
}

pub fn eval(input: &str) -> Result<f64, String> {
    // Vírgula decimal PT vira ponto (o app oficial aceita as duas).
    let src: String = input
        .chars()
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    let toks = tokenize(&src)?;
    let rpn = to_postfix(&toks)?;
    eval_rpn(&rpn)
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    let mut expect_operand = true;

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '0'..='9' | '.' => {
                if !expect_operand {
                    return Err("número inesperado".into());
                }
                let mut n = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        n.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let v: f64 = n.parse().map_err(|_| "número inválido")?;
                out.push(Tok::Num(v));
                expect_operand = false;
            }
            '(' => {
                if !expect_operand {
                    return Err("parêntese inesperado".into());
                }
                out.push(Tok::L);
                chars.next();
            }
            ')' => {
                if expect_operand {
                    return Err("parêntese inesperado".into());
                }
                out.push(Tok::R);
                chars.next();
            }
            '+' | '-' | '*' | '/' | '%' | '^' => {
                let t = match c {
                    '+' => Tok::Add,
                    '-' => Tok::Sub,
                    '*' => Tok::Mul,
                    '/' => Tok::Div,
                    '%' => Tok::Mod,
                    '^' => Tok::Pow,
                    _ => unreachable!(),
                };
                if expect_operand {
                    match c {
                        '-' => out.push(Tok::Neg),
                        '+' => {} // + unário é no-op
                        _ => return Err(format!("operador '{c}' sem operando à esquerda")),
                    }
                } else {
                    out.push(t);
                    expect_operand = true;
                }
                chars.next();
            }
            _ => return Err(format!("caractere inválido: '{c}'")),
        }
    }

    if out.is_empty() {
        return Err("expressão vazia".into());
    }
    Ok(out)
}

fn to_postfix(toks: &[Tok]) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut stack: Vec<Tok> = Vec::new();

    for &t in toks {
        match t {
            Tok::Num(_) => out.push(t),
            Tok::L => stack.push(t),
            Tok::R => loop {
                match stack.pop() {
                    Some(Tok::L) => break,
                    Some(op) => out.push(op),
                    None => return Err("parêntese não aberto".into()),
                }
            },
            op => {
                while let Some(&top) = stack.last() {
                    if !is_op(top) {
                        break;
                    }
                    if prec(top) > prec(op) || (prec(top) == prec(op) && !right_assoc(op)) {
                        out.push(stack.pop().unwrap());
                    } else {
                        break;
                    }
                }
                stack.push(op);
            }
        }
    }

    while let Some(t) = stack.pop() {
        if t == Tok::L {
            return Err("parêntese não fechado".into());
        }
        out.push(t);
    }
    Ok(out)
}

fn eval_rpn(rpn: &[Tok]) -> Result<f64, String> {
    fn pop(st: &mut Vec<f64>) -> Result<f64, String> {
        st.pop().ok_or_else(|| "expressão incompleta".to_string())
    }

    let mut st: Vec<f64> = Vec::new();
    for &t in rpn {
        match t {
            Tok::Num(v) => st.push(v),
            Tok::Neg => {
                let a = pop(&mut st)?;
                st.push(-a);
            }
            Tok::Add | Tok::Sub | Tok::Mul | Tok::Div | Tok::Mod | Tok::Pow => {
                let b = pop(&mut st)?;
                let a = pop(&mut st)?;
                let v = match t {
                    Tok::Add => a + b,
                    Tok::Sub => a - b,
                    Tok::Mul => a * b,
                    Tok::Div => a / b,
                    Tok::Mod => a % b,
                    Tok::Pow => a.powf(b),
                    _ => unreachable!(),
                };
                st.push(v);
            }
            Tok::L | Tok::R => return Err("parêntese inesperado".into()),
        }
    }

    if st.len() != 1 {
        return Err("expressão incompleta".into());
    }
    Ok(st[0])
}

/// Formata sem ruído binário: 0.1+0.2 vira "0.3" (o app oficial se garante
/// disso; aqui, casas fixas + trim já resolvem o caso comum).
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str, want: f64) {
        let got = eval(src).unwrap();
        assert!((got - want).abs() < 1e-9, "{src} = {got}, esperava {want}");
    }

    #[test]
    fn aritmetica_basica() {
        ok("1+2", 3.0);
        ok("1+2*3", 7.0);
        ok("(1+2)*3", 9.0);
        ok("10/4", 2.5);
        ok("7-10", -3.0);
    }

    #[test]
    fn precedencia_e_associatividade() {
        ok("2^10", 1024.0);
        ok("2^3^2", 512.0); // ^ é right-assoc: 2^(3^2)
        ok("-2^2", -4.0); // menos unário abaixo do ^
        ok("10%3", 1.0);
    }

    #[test]
    fn unario() {
        ok("-5+3", -2.0);
        ok("2*-3", -6.0);
        ok("--5", 5.0);
        ok("+5", 5.0);
    }

    #[test]
    fn virgula_decimal_pt() {
        ok("1,5+1", 2.5);
        ok("2,5*4", 10.0);
    }

    #[test]
    fn espacos() {
        ok("  2 + 3  ", 5.0);
        ok("2 + 3 * ( 4 - 1 )", 11.0);
    }

    #[test]
    fn erros() {
        assert!(eval("").is_err());
        assert!(eval("   ").is_err());
        assert!(eval("1+").is_err());
        assert!(eval("(1+2").is_err());
        assert!(eval("1+2)").is_err());
        assert!(eval("1 2").is_err());
        assert!(eval("2(3)").is_err());
        assert!(eval("()").is_err());
        assert!(eval("abc").is_err());
        assert!(eval("1..2").is_err());
    }

    #[test]
    fn formato_sem_ruido() {
        assert_eq!(fmt_num(0.1 + 0.2), "0.3");
        assert_eq!(fmt_num(42.0), "42");
        assert_eq!(fmt_num(2.0 / 3.0), "0.6666666667");
        assert_eq!(fmt_num(f64::INFINITY), "—");
    }
}
