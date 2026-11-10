//! 词法分析器 (Lexer)
//!
//! 将 JavaScript 源码切分为 Token 流
//! Phase A: 支持箭头函数、const、模板字符串、switch、三元、import/export

mod token;
pub use token::*;

/// 将源码切分为 Token 序列
pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    let mut line = 1;
    let mut col = 1;

    while let Some(&ch) = chars.peek() {
        match ch {
            // 跳过空白
            ' ' | '\t' | '\r' => {
                chars.next();
                col += 1;
            }
            '\n' => {
                chars.next();
                line += 1;
                col = 1;
            }
            // 跳过单行注释
            '/' if matches!(chars.clone().nth(1), Some('/')) => {
                while let Some(&c) = chars.peek() {
                    if c == '\n' { break; }
                    chars.next();
                }
            }
            // 跳过块注释 /* ... */
            '/' if matches!(chars.clone().nth(1), Some('*')) => {
                let start_line = line;
                let start_col = col;
                chars.next(); chars.next(); // consume /*
                col += 2;
                loop {
                    match chars.peek().copied() {
                        Some('*') if matches!(chars.clone().nth(1), Some('/')) => {
                            chars.next(); chars.next();
                            col += 2;
                            break;
                        }
                        Some('\n') => { chars.next(); line += 1; col = 1; }
                        Some(_) => { chars.next(); col += 1; }
                        None => {
                            return Err(format!("行{} 列{}: 未闭合的块注释 /*", start_line, start_col));
                        }
                    }
                }
            }
            // 字符串字面量：双引号 " 与单引号 ' 等价处理
            quote @ ('"' | '\'') => {
                let start_col = col;
                chars.next(); // consume opening quote
                col += 1;
                let mut s = String::new();
                loop {
                    match chars.peek() {
                        Some(&c) if c == quote => {
                            chars.next();
                            col += 1;
                            break;
                        }
                        Some(&'\\') => {
                            chars.next();
                            col += 1;
                            match chars.peek() {
                                Some(&'n') => { s.push('\n'); chars.next(); col += 1; }
                                Some(&'t') => { s.push('\t'); chars.next(); col += 1; }
                                Some(&'\\') => { s.push('\\'); chars.next(); col += 1; }
                                Some(&'"') => { s.push('"'); chars.next(); col += 1; }
                                Some(&'\'') => { s.push('\''); chars.next(); col += 1; }
                                _ => s.push('\\'),
                            }
                        }
                        Some(&c) => {
                            s.push(c);
                            chars.next();
                            col += 1;
                        }
                        None => {
                            return Err(format!("行{} 列{}: 未闭合的字符串", line, start_col));
                        }
                    }
                }
                tokens.push(Token { kind: TokenKind::StringLit(s), line, col: start_col });
            }
            // 模板字符串 (Phase A.2)
            '`' => {
                let start_col = col;
                chars.next();
                col += 1;
                let mut parts: Vec<TemplatePart> = Vec::new();
                let mut current_text = String::new();
                loop {
                    let peeked = chars.peek().copied();
                    match peeked {
                        Some('`') => {
                            chars.next();
                            col += 1;
                            if !current_text.is_empty() {
                                parts.push(TemplatePart::Text(current_text.clone()));
                                current_text.clear();
                            }
                            break;
                        }
                        Some('$') if matches!(chars.clone().nth(1), Some('{')) => {
                            chars.next(); col += 1; // $
                            chars.next(); col += 1; // {
                            if !current_text.is_empty() {
                                parts.push(TemplatePart::Text(current_text.clone()));
                                current_text.clear();
                            }
                            let mut expr = String::new();
                            let mut depth = 1;
                            while let Some(&c) = chars.peek() {
                                if c == '{' { depth += 1; }
                                if c == '}' { depth -= 1; }
                                if depth == 0 { chars.next(); col += 1; break; }
                                expr.push(c);
                                chars.next();
                                col += 1;
                            }
                            parts.push(TemplatePart::Expr(expr));
                        }
                        Some(c) => {
                            current_text.push(c);
                            chars.next();
                            col += 1;
                            if c == '\n' { line += 1; col = 1; }
                        }
                        None => {
                            return Err(format!("行{} 列{}: 未闭合的模板字符串", line, start_col));
                        }
                    }
                }
                tokens.push(Token { kind: TokenKind::TemplateLit(parts), line, col: start_col });
            }
            // 数字字面量（十进制 / 十六进制 0x）
            '0'..='9' => {
                let start_col = col;
                let mut num_str = String::new();
                // 十六进制：0x / 0X 前缀
                if ch == '0' && matches!(chars.clone().nth(1), Some('x') | Some('X')) {
                    chars.next(); chars.next(); // consume 0x
                    col += 2;
                    let mut digits = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_hexdigit() {
                            digits.push(c);
                            chars.next();
                            col += 1;
                        } else {
                            break;
                        }
                    }
                    if digits.is_empty() {
                        return Err(format!("行{} 列{}: 十六进制字面量缺少数字 '0x'", line, start_col));
                    }
                    let value: i64 = i64::from_str_radix(&digits, 16).map_err(|_| {
                        format!("行{} 列{}: 无效十六进制 '0x{}'", line, start_col, digits)
                    })?;
                    tokens.push(Token { kind: TokenKind::Number(value), line, col: start_col });
                    continue;
                }
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        chars.next();
                        col += 1;
                    } else {
                        break;
                    }
                }
                let mut value: i64 = num_str.parse().map_err(|_| {
                    format!("行{} 列{}: 无效数字 '{}'", line, start_col, num_str)
                })?;
                // 定点小数：小数字面量统一按 ×100 缩放（最多 2 位小数）
                // 3.14 → 314，1.5 → 150；输出时需自行 /100 还原
                if chars.peek() == Some(&'.') && matches!(chars.clone().nth(1), Some(c) if c.is_ascii_digit()) {
                    chars.next(); // consume '.'
                    col += 1;
                    let mut frac = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            frac.push(c);
                            chars.next();
                            col += 1;
                        } else {
                            break;
                        }
                    }
                    if frac.len() > 2 {
                        return Err(format!(
                            "行{} 列{}: 定点小数仅支持最多 2 位小数（统一 ×100 缩放），实际 {} 位 '{}.{}'",
                            line, start_col, frac.len(), num_str, frac));
                    }
                    let frac_val: i64 = if frac.len() == 1 {
                        format!("{}0", frac).parse().unwrap()
                    } else {
                        frac.parse().unwrap()
                    };
                    value = value.checked_mul(100).ok_or_else(|| format!(
                        "行{} 列{}: 定点小数整数部分溢出 '{}.'", line, start_col, num_str))?;
                    value += frac_val;
                }
                tokens.push(Token { kind: TokenKind::Number(value), line, col: start_col });
            }
            // 标识符 / 关键字
            'a'..='z' | 'A'..='Z' | '_' => {
                let start_col = col;
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                        chars.next();
                        col += 1;
                    } else {
                        break;
                    }
                }
                let kind = match ident.as_str() {
                    "let" => TokenKind::Let,
                    "const" => TokenKind::Const,
                    "function" => TokenKind::Function,
                    "if" => TokenKind::If,
                    "else" => TokenKind::Else,
                    "while" => TokenKind::While,
                    "for" => TokenKind::For,
                    "return" => TokenKind::Return,
                    "switch" => TokenKind::Switch,
                    "case" => TokenKind::Case,
                    "default" => TokenKind::Default,
                    "break" => TokenKind::Break,
                    "continue" => TokenKind::Continue,
                    "import" => TokenKind::Import,
                    "export" => TokenKind::Export,
                    "from" => TokenKind::From,
                    "true" => TokenKind::True,
                    "false" => TokenKind::False,
                    "null" => TokenKind::Null,
                    "async" => TokenKind::Async,
                    "await" => TokenKind::Await,
                    _ => TokenKind::Ident(ident),
                };
                tokens.push(Token { kind, line, col: start_col });
            }
            // 运算符和分隔符
            '+' => {
                chars.next();
                if chars.peek() == Some(&'+') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::PlusPlus, line, col });
                    col += 2;
                } else if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::PlusEq, line, col });
                    col += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Plus, line, col });
                    col += 1;
                }
            }
            '-' => {
                chars.next();
                if chars.peek() == Some(&'-') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::MinusMinus, line, col });
                    col += 2;
                } else if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::MinusEq, line, col });
                    col += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Minus, line, col });
                    col += 1;
                }
            }
            '*' => {
                let start_col = col;
                chars.next(); col += 1;
                if chars.peek() == Some(&'=') {
                    chars.next(); col += 1;
                    tokens.push(Token { kind: TokenKind::StarEq, line, col: start_col });
                } else {
                    tokens.push(Token { kind: TokenKind::Star, line, col: start_col });
                }
            }
            '/' => {
                let start_col = col;
                chars.next(); col += 1;
                if chars.peek() == Some(&'=') {
                    chars.next(); col += 1;
                    tokens.push(Token { kind: TokenKind::SlashEq, line, col: start_col });
                } else {
                    tokens.push(Token { kind: TokenKind::Slash, line, col: start_col });
                }
            }
            '%' => {
                let start_col = col;
                chars.next(); col += 1;
                if chars.peek() == Some(&'=') {
                    chars.next(); col += 1;
                    tokens.push(Token { kind: TokenKind::PercentEq, line, col: start_col });
                } else {
                    tokens.push(Token { kind: TokenKind::Percent, line, col: start_col });
                }
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token { kind: TokenKind::EqEqEq, line, col });
                        col += 3;
                    } else {
                        tokens.push(Token { kind: TokenKind::EqEq, line, col });
                        col += 2;
                    }
                } else if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::Arrow, line, col });
                    col += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Eq, line, col });
                    col += 1;
                }
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(Token { kind: TokenKind::NotEqEq, line, col });
                        col += 3;
                    } else {
                        tokens.push(Token { kind: TokenKind::NotEq, line, col });
                        col += 2;
                    }
                } else {
                    tokens.push(Token { kind: TokenKind::Bang, line, col });
                    col += 1;
                }
            }
            '&' => {
                chars.next();
                if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::AmpAmp, line, col });
                    col += 2;
                } else if chars.peek() == Some(&'=') {
                    return Err(format!("行{} 列{}: 复合赋值 '&=' 尚未实现（请写 x = x & 值）", line, col));
                } else {
                    // 位运算债清偿: 单 & = 按位与
                    tokens.push(Token { kind: TokenKind::Amp, line, col });
                    col += 1;
                }
            }
            '|' => {
                chars.next();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::PipePipe, line, col });
                    col += 2;
                } else if chars.peek() == Some(&'=') {
                    return Err(format!("行{} 列{}: 复合赋值 '|=' 尚未实现（请写 x = x | 值）", line, col));
                } else {
                    // 位运算债清偿: 单 | = 按位或
                    tokens.push(Token { kind: TokenKind::Pipe, line, col });
                    col += 1;
                }
            }
            '^' => {
                // 位运算债清偿: 按位异或
                tokens.push(Token { kind: TokenKind::Caret, line, col });
                chars.next(); col += 1;
            }
            '~' => {
                // 位运算债清偿: 按位取反（一元）
                tokens.push(Token { kind: TokenKind::Tilde, line, col });
                chars.next(); col += 1;
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'<') {
                    chars.next();
                    if chars.peek() == Some(&'=') {
                        return Err(format!("行{} 列{}: 复合赋值 '<<=' 尚未实现（请写 x = x << 值）", line, col));
                    }
                    // 位运算债清偿: 左移
                    tokens.push(Token { kind: TokenKind::Shl, line, col });
                    col += 2;
                } else if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::Le, line, col });
                    col += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Lt, line, col });
                    col += 1;
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        if chars.peek() == Some(&'=') {
                            return Err(format!("行{} 列{}: 复合赋值 '>>>=' 尚未实现（请写 x = x >>> 值）", line, col));
                        }
                        // 位运算债清偿: 无符号右移
                        tokens.push(Token { kind: TokenKind::Shrrr, line, col });
                        col += 3;
                    } else if chars.peek() == Some(&'=') {
                        return Err(format!("行{} 列{}: 复合赋值 '>>=' 尚未实现（请写 x = x >> 值）", line, col));
                    } else {
                        // 位运算债清偿: 右移
                        tokens.push(Token { kind: TokenKind::Shr, line, col });
                        col += 2;
                    }
                } else if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::Ge, line, col });
                    col += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Gt, line, col });
                    col += 1;
                }
            }
            '(' => { tokens.push(Token { kind: TokenKind::LParen, line, col }); chars.next(); col += 1; }
            ')' => { tokens.push(Token { kind: TokenKind::RParen, line, col }); chars.next(); col += 1; }
            '{' => { tokens.push(Token { kind: TokenKind::LBrace, line, col }); chars.next(); col += 1; }
            '}' => { tokens.push(Token { kind: TokenKind::RBrace, line, col }); chars.next(); col += 1; }
            '[' => { tokens.push(Token { kind: TokenKind::LBracket, line, col }); chars.next(); col += 1; }
            ']' => { tokens.push(Token { kind: TokenKind::RBracket, line, col }); chars.next(); col += 1; }
            ';' => { tokens.push(Token { kind: TokenKind::Semi, line, col }); chars.next(); col += 1; }
            ',' => { tokens.push(Token { kind: TokenKind::Comma, line, col }); chars.next(); col += 1; }
            '.' => { tokens.push(Token { kind: TokenKind::Dot, line, col }); chars.next(); col += 1; }
            ':' => { tokens.push(Token { kind: TokenKind::Colon, line, col }); chars.next(); col += 1; }
            '?' => { tokens.push(Token { kind: TokenKind::Question, line, col }); chars.next(); col += 1; }
            '@' => { tokens.push(Token { kind: TokenKind::At, line, col }); chars.next(); col += 1; }
            _ => {
                return Err(format!("行{} 列{}: 未识别字符 '{}'", line, col, ch));
            }
        }
    }

    tokens.push(Token { kind: TokenKind::Eof, line, col });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_let_statement() {
        let tokens = tokenize("let a = 10;").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[1].kind, TokenKind::Ident("a".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Eq);
        assert_eq!(tokens[3].kind, TokenKind::Number(10));
        assert_eq!(tokens[4].kind, TokenKind::Semi);
    }

    #[test]
    fn test_arithmetic() {
        let tokens = tokenize("a + b * 2").unwrap();
        assert_eq!(tokens.len(), 6); // a + b * 2 EOF
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("let s = \"hello\";").unwrap();
        assert_eq!(tokens[3].kind, TokenKind::StringLit("hello".to_string()));
    }

    #[test]
    fn test_dot_access() {
        let tokens = tokenize("console.log(42);").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("console".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Ident("log".to_string()));
    }

    #[test]
    fn test_array_brackets() {
        let tokens = tokenize("let arr = [1, 2, 3];").unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::LBracket));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::RBracket));
    }

    // Phase v1.5: async/await 词法测试
    #[test]
    fn test_lex_async_function() {
        let tokens = tokenize("async function foo() {}").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Async);
        assert_eq!(tokens[1].kind, TokenKind::Function);
        assert_eq!(tokens[2].kind, TokenKind::Ident("foo".to_string()));
    }

    #[test]
    fn test_lex_await_expr() {
        let tokens = tokenize("await sensor.read()").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Await);
        assert_eq!(tokens[1].kind, TokenKind::Ident("sensor".to_string()));
    }

    #[test]
    fn test_lex_async_as_ident() {
        // async 在非函数上下文中作为标识符
        let tokens = tokenize("let async = 5;").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[1].kind, TokenKind::Async);
        assert_eq!(tokens[2].kind, TokenKind::Eq);
    }

    // 语言完备性批 1：单引号 / 块注释 / 十六进制 / 复合赋值
    #[test]
    fn test_lex_single_quote_string() {
        let tokens = tokenize("let s = 'hi';").unwrap();
        assert_eq!(tokens[3].kind, TokenKind::StringLit("hi".to_string()));
    }

    #[test]
    fn test_lex_single_quote_escape() {
        let tokens = tokenize("let s = 'it\\'s';").unwrap();
        assert_eq!(tokens[3].kind, TokenKind::StringLit("it's".to_string()));
    }

    #[test]
    fn test_lex_block_comment() {
        let tokens = tokenize("a /* 跨行\n注释 */ b").unwrap();
        assert_eq!(tokens.len(), 3); // a b EOF
        assert_eq!(tokens[0].kind, TokenKind::Ident("a".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Ident("b".to_string()));
    }

    #[test]
    fn test_lex_block_comment_unclosed() {
        assert!(tokenize("a /* 未闭合").is_err());
    }

    #[test]
    fn test_lex_hex_number() {
        let tokens = tokenize("let x = 0xFF;").unwrap();
        assert_eq!(tokens[3].kind, TokenKind::Number(255));
        let tokens = tokenize("0x1a").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Number(26));
    }

    #[test]
    fn test_lex_compound_assign_tokens() {
        let tokens = tokenize("a += b -= c *= d /= e %= f").unwrap();
        assert_eq!(tokens[1].kind, TokenKind::PlusEq);
        assert_eq!(tokens[3].kind, TokenKind::MinusEq);
        assert_eq!(tokens[5].kind, TokenKind::StarEq);
        assert_eq!(tokens[7].kind, TokenKind::SlashEq);
        assert_eq!(tokens[9].kind, TokenKind::PercentEq);
    }

    #[test]
    fn test_lex_minus_minus_vs_minus_eq() {
        let tokens = tokenize("a--;").unwrap();
        assert_eq!(tokens[1].kind, TokenKind::MinusMinus);
        let tokens = tokenize("a -= 1;").unwrap();
        assert_eq!(tokens[1].kind, TokenKind::MinusEq);
    }

    // 语言完备性批 2：定点小数字面量（×100 缩放）
    #[test]
    fn test_lex_fixed_point_two_digits() {
        let tokens = tokenize("let x = 3.14;").unwrap();
        assert_eq!(tokens[3].kind, TokenKind::Number(314));
    }

    #[test]
    fn test_lex_fixed_point_one_digit_pads() {
        let tokens = tokenize("1.5").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Number(150));
        let tokens = tokenize("0.05").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Number(5));
    }

    #[test]
    fn test_lex_fixed_point_too_many_digits() {
        assert!(tokenize("1.234").is_err());
    }

    #[test]
    fn test_lex_dot_after_int_not_digit_is_not_decimal() {
        // "3." 后面不是数字：仍按整数 + Dot 处理，不报错不吃字符
        let tokens = tokenize("let a = [1, 2]; a[0].x;").unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Dot));
    }
}
