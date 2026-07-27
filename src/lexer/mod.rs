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
            // 字符串字面量 (Phase 4)
            '"' => {
                let start_col = col;
                chars.next(); // consume opening quote
                col += 1;
                let mut s = String::new();
                loop {
                    match chars.peek() {
                        Some(&'"') => {
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
            // 数字字面量
            '0'..='9' => {
                let start_col = col;
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        chars.next();
                        col += 1;
                    } else {
                        break;
                    }
                }
                let value: i64 = num_str.parse().map_err(|_| {
                    format!("行{} 列{}: 无效数字 '{}'", line, start_col, num_str)
                })?;
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
                } else {
                    tokens.push(Token { kind: TokenKind::Minus, line, col });
                    col += 1;
                }
            }
            '*' => { tokens.push(Token { kind: TokenKind::Star, line, col }); chars.next(); col += 1; }
            '/' => { tokens.push(Token { kind: TokenKind::Slash, line, col }); chars.next(); col += 1; }
            '%' => { tokens.push(Token { kind: TokenKind::Percent, line, col }); chars.next(); col += 1; }
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
                } else {
                    return Err(format!("行{} 列{}: 未识别字符 '&'（是否需要 '&&'？）", line, col));
                }
            }
            '|' => {
                chars.next();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Token { kind: TokenKind::PipePipe, line, col });
                    col += 2;
                } else {
                    return Err(format!("行{} 列{}: 未识别字符 '|'（是否需要 '||'？）", line, col));
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
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
                if chars.peek() == Some(&'=') {
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
}
