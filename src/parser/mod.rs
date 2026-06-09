//! 语法分析器 (Parser)
//!
//! 将 Token 流解析为 AST
//! Phase 4: 支持 console.log、数组、取模、逻辑运算

mod ast;
pub use ast::*;

use crate::lexer::{Token, TokenKind};

/// 解析 Token 流为 AST
pub fn parse(tokens: &[Token]) -> Result<Program, String> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<(), String> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            let tok = &self.tokens[self.pos];
            Err(format!("行{} 列{}: 期望 {:?}, 得到 {:?}", tok.line, tok.col, expected, tok.kind))
        }
    }

    fn parse_program(&mut self) -> Result<Program, String> {
        let mut statements = Vec::new();
        let mut functions = Vec::new();
        
        while *self.peek() != TokenKind::Eof {
            if *self.peek() == TokenKind::Function {
                functions.push(self.parse_function()?);
            } else {
                statements.push(self.parse_statement()?);
            }
        }
        Ok(Program { statements, functions })
    }

    fn parse_function(&mut self) -> Result<Function, String> {
        self.advance(); // consume 'function'
        
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => { self.advance(); n }
            _ => {
                let tok = &self.tokens[self.pos];
                return Err(format!("行{} 列{}: function 后期望标识符", tok.line, tok.col));
            }
        };
        
        self.expect(&TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        
        let body = self.parse_block()?;
        
        Ok(Function { name, params, body })
    }

    fn parse_params(&mut self) -> Result<Vec<String>, String> {
        let mut params = Vec::new();
        
        if *self.peek() == TokenKind::RParen {
            return Ok(params);
        }
        
        loop {
            let param = match self.peek().clone() {
                TokenKind::Ident(n) => { self.advance(); n }
                _ => {
                    let tok = &self.tokens[self.pos];
                    return Err(format!("行{} 列{}: 期望参数名", tok.line, tok.col));
                }
            };
            params.push(param);
            
            if *self.peek() == TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }
        
        Ok(params)
    }

    fn parse_statement(&mut self) -> Result<Statement, String> {
        match self.peek() {
            TokenKind::Let => self.parse_let(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::Ident(_) => {
                let name = if let TokenKind::Ident(n) = self.peek().clone() {
                    n.clone()
                } else {
                    unreachable!()
                };
                
                // 检查是否是 console.log(...)
                if name == "console" {
                    return self.parse_console_log();
                }
                
                self.advance(); // consume ident
                
                // 数组索引赋值: name[index] = value;
                if *self.peek() == TokenKind::LBracket {
                    self.advance(); // consume '['
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    self.expect(&TokenKind::Eq)?;
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::IndexAssign { name, index, value });
                }
                
                // 函数调用: ident(...)
                if *self.peek() == TokenKind::LParen {
                    self.advance(); // consume '('
                    let args = self.parse_args()?;
                    self.expect(&TokenKind::RParen)?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::Expr(Expr::Call { name, args }));
                }
                
                // 赋值: ident = expr;
                if *self.peek() == TokenKind::Eq {
                    self.advance(); // consume '='
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::Assign { name, value });
                }
                
                // 表达式语句
                let mut expr = Expr::Ident(name);
                expr = self.parse_expr_continuation(expr)?;
                self.expect(&TokenKind::Semi)?;
                Ok(Statement::Expr(expr))
            }
            _ => {
                let tok = &self.tokens[self.pos];
                Err(format!("行{} 列{}: 意外的 token {:?}", tok.line, tok.col, tok.kind))
            }
        }
    }

    /// 解析 console.log(args...);
    fn parse_console_log(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'console'
        self.expect(&TokenKind::Dot)?;
        // 期望 'log'
        match self.peek().clone() {
            TokenKind::Ident(ref n) if n == "log" => { self.advance(); }
            _ => {
                let tok = &self.tokens[self.pos];
                return Err(format!("行{} 列{}: console 后期望 .log", tok.line, tok.col));
            }
        }
        self.expect(&TokenKind::LParen)?;
        let args = self.parse_args()?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::ConsoleLog(args))
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        
        if *self.peek() == TokenKind::RParen {
            return Ok(args);
        }
        
        loop {
            args.push(self.parse_expr()?);
            
            if *self.peek() == TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }
        
        Ok(args)
    }

    fn parse_let(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'let'
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => { self.advance(); n }
            _ => {
                let tok = &self.tokens[self.pos];
                return Err(format!("行{} 列{}: let 后期望标识符", tok.line, tok.col));
            }
        };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::Let { name, value })
    }

    fn parse_return(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'return'
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::Return(value))
    }

    fn parse_if(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'if'
        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;
        let then_body = self.parse_block()?;
        let else_body = if *self.peek() == TokenKind::Else {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(Statement::If { condition, then_body, else_body })
    }

    fn parse_while(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'while'
        self.expect(&TokenKind::LParen)?;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;
        let body = self.parse_block()?;
        Ok(Statement::While { condition, body })
    }

    fn parse_block(&mut self) -> Result<Vec<Statement>, String> {
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(stmts)
    }

    /// 表达式解析（运算符优先级）
    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_logical_or()
    }

    /// 继续解析表达式（已有左侧部分）
    fn parse_expr_continuation(&mut self, left: Expr) -> Result<Expr, String> {
        // 检查是否有数组索引
        let left = self.parse_postfix(left)?;
        self.parse_logical_or_continuation(left)
    }

    // Phase 4: 逻辑或 (最低优先级)
    fn parse_logical_or(&mut self) -> Result<Expr, String> {
        let left = self.parse_logical_and()?;
        self.parse_logical_or_continuation(left)
    }

    fn parse_logical_or_continuation(&mut self, mut left: Expr) -> Result<Expr, String> {
        while *self.peek() == TokenKind::PipePipe {
            self.advance();
            let right = self.parse_logical_and()?;
            left = Expr::BinOp { op: BinOp::Or, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;
        while *self.peek() == TokenKind::AmpAmp {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinOp { op: BinOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::NotEq,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        let primary = self.parse_primary()?;
        self.parse_postfix(primary)
    }

    /// 后缀操作符：数组索引 arr[i]
    fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr, String> {
        while *self.peek() == TokenKind::LBracket {
            self.advance(); // consume '['
            let index = self.parse_expr()?;
            self.expect(&TokenKind::RBracket)?;
            expr = Expr::Index { array: Box::new(expr), index: Box::new(index) };
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            TokenKind::Number(n) => { self.advance(); Ok(Expr::Number(n)) }
            TokenKind::StringLit(s) => { self.advance(); Ok(Expr::StringLit(s)) }
            TokenKind::Ident(name) => {
                self.advance();
                // 函数调用
                if *self.peek() == TokenKind::LParen {
                    self.advance(); // consume '('
                    let args = self.parse_args()?;
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            // Phase 4: 数组字面量 [expr, expr, ...]
            TokenKind::LBracket => {
                self.advance(); // consume '['
                let mut elements = Vec::new();
                if *self.peek() != TokenKind::RBracket {
                    loop {
                        elements.push(self.parse_expr()?);
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::Array(elements))
            }
            _ => {
                let tok = &self.tokens[self.pos];
                Err(format!("行{} 列{}: 期望表达式, 得到 {:?}", tok.line, tok.col, tok.kind))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn test_parse_let() {
        let tokens = tokenize("let x = 42;").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement::Let { name, value } => {
                assert_eq!(name, "x");
                assert_eq!(*value, Expr::Number(42));
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_expr() {
        let tokens = tokenize("let c = a + b;").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn test_parse_function() {
        let tokens = tokenize("function add(a, b) { return a + b; }").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
        assert_eq!(program.functions[0].params, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_call() {
        let tokens = tokenize("let x = add(3, 4);").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0] {
            Statement::Let { value, .. } => {
                match value {
                    Expr::Call { name, args } => {
                        assert_eq!(name, "add");
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("期望函数调用"),
                }
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_console_log() {
        let tokens = tokenize("console.log(42);").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0] {
            Statement::ConsoleLog(args) => {
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], Expr::Number(42));
            }
            _ => panic!("期望 ConsoleLog 语句"),
        }
    }

    #[test]
    fn test_parse_console_log_string() {
        let tokens = tokenize("console.log(\"hello\");").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0] {
            Statement::ConsoleLog(args) => {
                assert_eq!(args[0], Expr::StringLit("hello".to_string()));
            }
            _ => panic!("期望 ConsoleLog 语句"),
        }
    }

    #[test]
    fn test_parse_array() {
        let tokens = tokenize("let arr = [1, 2, 3];").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0] {
            Statement::Let { value, .. } => {
                match value {
                    Expr::Array(elems) => assert_eq!(elems.len(), 3),
                    _ => panic!("期望数组字面量"),
                }
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_array_index() {
        let tokens = tokenize("let x = arr[0];").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0] {
            Statement::Let { value, .. } => {
                assert!(matches!(value, Expr::Index { .. }));
            }
            _ => panic!("期望 Let 语句"),
        }
    }
}
