//! 语法分析器 (Parser)
//!
//! 将 Token 流解析为 AST
//! Phase A: 箭头函数、const、模板字符串、switch、三元、import/export、bool/null

mod ast;
pub use ast::*;  // Phase 6: SourceStatement, SourceFunction

use crate::lexer::{Token, TokenKind, TemplatePart};

/// Phase 3.1: 注解信息
#[derive(Debug, Clone)]
struct Annotation {
    name: String,
    value: Option<u32>,       // 数值参数 (wcet_limit / priority / period_us)
    str_value: Option<String>, // 字符串参数 (timer name)
}

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
            let line = self.tokens[self.pos].line;
            match self.peek() {
                TokenKind::Function => {
                    let func = self.parse_function()?;
                    functions.push(SourceFunction { func, line });
                }
                // Phase v1.5: async function 声明
                TokenKind::Async => {
                    self.advance(); // consume 'async'
                    if *self.peek() == TokenKind::Function {
                        let mut func = self.parse_function()?;
                        func.is_async = true;
                        functions.push(SourceFunction { func, line });
                    } else {
                        // async 作为标识符（上下文关键字）
                        let stmt = self.parse_statement_after_ident("async".to_string())?;
                        statements.push(SourceStatement { stmt, line });
                    }
                }
                TokenKind::Import => {
                    let stmt = self.parse_import()?;
                    statements.push(SourceStatement { stmt, line });
                }
                TokenKind::Export => {
                    let stmt = self.parse_export()?;
                    statements.push(SourceStatement { stmt, line });
                }
                // Phase 3.1: @realtime / @interrupt / @priority 注解
                TokenKind::At => {
                    let annotations = self.parse_annotations()?;
                    // 注解后必须是 function 或 async function
                    let mut func = if *self.peek() == TokenKind::Async {
                        self.advance(); // consume 'async'
                        let mut f = self.parse_function()?;
                        f.is_async = true;
                        f
                    } else if *self.peek() == TokenKind::Function {
                        self.parse_function()?
                    } else {
                        return Err(format!("行{}: 注解后必须是函数声明", line));
                    };
                    // 应用注解
                    for ann in &annotations {
                        match ann.name.as_str() {
                            "realtime" => {
                                func.is_realtime = true;
                                func.wcet_limit_us = ann.value;
                            }
                            "interrupt" => {
                                func.interrupt_timer = ann.str_value.clone();
                                func.interrupt_period_us = ann.value;
                            }
                            "priority" => {
                                func.priority = ann.value.map(|v| v as u8);
                            }
                            _ => {}
                        }
                    }
                    functions.push(SourceFunction { func, line });
                }
                _ => {
                    let stmt = self.parse_statement()?;
                    statements.push(SourceStatement { stmt, line });
                }
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
        
        Ok(Function { name, params, body, is_async: false, is_realtime: false, wcet_limit_us: None, interrupt_timer: None, interrupt_period_us: None, priority: None })
    }

    /// Phase 3.1: 解析注解序列 (@realtime, @interrupt, @priority)
    fn parse_annotations(&mut self) -> Result<Vec<Annotation>, String> {
        let mut annotations = Vec::new();
        while *self.peek() == TokenKind::At {
            self.advance(); // consume '@'
            let name = match self.peek().clone() {
                TokenKind::Ident(n) => { self.advance(); n }
                _ => {
                    let tok = &self.tokens[self.pos];
                    return Err(format!("行{} 列{}: @ 后期望注解名", tok.line, tok.col));
                }
            };
            let mut ann = Annotation { name, value: None, str_value: None };
            // 解析可选参数 (...)
            if *self.peek() == TokenKind::LParen {
                self.advance(); // consume '('
                // 解析参数列表
                loop {
                    match self.peek().clone() {
                        TokenKind::RParen => { self.advance(); break; }
                        TokenKind::Number(n) => {
                            self.advance();
                            ann.value = Some(n as u32);
                        }
                        TokenKind::StringLit(s) => {
                            self.advance();
                            ann.str_value = Some(s);
                        }
                        TokenKind::Ident(key) => {
                            self.advance();
                            // key: value 形式 (period_us: 100)
                            if *self.peek() == TokenKind::Colon {
                                self.advance(); // consume ':'
                                if let TokenKind::Number(v) = self.peek().clone() {
                                    self.advance();
                                    if key == "period_us" {
                                        ann.value = Some(v as u32);
                                    }
                                }
                            }
                        }
                        TokenKind::Comma => { self.advance(); }
                        _ => { self.advance(); } // skip unknown
                    }
                }
            }
            annotations.push(ann);
        }
        Ok(annotations)
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
            TokenKind::Const => self.parse_const(),
            TokenKind::Return => self.parse_return(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Switch => self.parse_switch(),
            TokenKind::Break => {
                self.advance();
                self.expect(&TokenKind::Semi)?;
                Ok(Statement::Break)
            }
            TokenKind::Continue => {
                self.advance();
                self.expect(&TokenKind::Semi)?;
                Ok(Statement::Continue)
            }
            TokenKind::Import => self.parse_import(),
            TokenKind::Export => self.parse_export(),
            // Phase 1.4: await expr; 作为语句 (async 函数内)
            TokenKind::Await => {
                self.advance(); // consume 'await'
                let expr = self.parse_unary()?;
                self.expect(&TokenKind::Semi)?;
                Ok(Statement::Expr(Expr::Await { expr: Box::new(expr) }))
            }
            // Phase v1.5: async function 在语句位置
            TokenKind::Async => {
                self.advance(); // consume 'async'
                if *self.peek() == TokenKind::Function {
                    let mut func = self.parse_function()?;
                    func.is_async = true;
                    // 语句位置的 async function 当作表达式语句
                    Ok(Statement::Expr(Expr::Ident(format!("__async_fn_{}", func.name))))
                } else {
                    // async 作为标识符
                    self.parse_statement_after_ident("async".to_string())
                }
            }
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
                
                // PostIncrement: name++
                if *self.peek() == TokenKind::PlusPlus {
                    self.advance();
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::PostInc(name));
                }
                
                // PostDecrement: name--
                if *self.peek() == TokenKind::MinusMinus {
                    self.advance();
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::PostDec(name));
                }
                
                // 数组索引赋值: name[index] = value; 或 name[index] op= value;
                if *self.peek() == TokenKind::LBracket {
                    self.advance(); // consume '['
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    let compound = Self::compound_binop(self.peek());
                    if compound.is_some() {
                        self.advance();
                    } else {
                        self.expect(&TokenKind::Eq)?;
                    }
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    let value = match compound {
                        Some(op) => Expr::BinOp {
                            op,
                            left: Box::new(Expr::Index { array: Box::new(Expr::Ident(name.clone())), index: Box::new(index.clone()) }),
                            right: Box::new(value),
                        },
                        None => value,
                    };
                    return Ok(Statement::IndexAssign { name, index, value });
                }
                
                // 属性赋值或方法调用: obj.prop = value; 或 obj.method(args);
                if *self.peek() == TokenKind::Dot {
                    self.advance(); // consume '.'
                    let prop = match self.peek().clone() {
                        TokenKind::Ident(p) => { self.advance(); p }
                        _ => return Err("prop: . expects ident".to_string()),
                    };
                    // 方法调用: obj.method(args)
                    if *self.peek() == TokenKind::LParen {
                        self.advance(); // consume '('
                        let call_name = format!("{}.{}", name, prop);
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        self.expect(&TokenKind::Semi)?;
                        return Ok(Statement::Expr(Expr::Call { name: call_name, args }));
                    }
                    // 属性赋值: obj.prop = value 或 obj.prop op= value
                    let compound = Self::compound_binop(self.peek());
                    if compound.is_some() {
                        self.advance();
                    } else {
                        self.expect(&TokenKind::Eq)?;
                    }
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    let value = match compound {
                        Some(op) => Expr::BinOp {
                            op,
                            left: Box::new(Expr::PropAccess { object: Box::new(Expr::Ident(name.clone())), property: prop.clone() }),
                            right: Box::new(value),
                        },
                        None => value,
                    };
                    return Ok(Statement::PropAssign { object: name, property: prop, value });
                }
                
                // 函数调用: ident(...)
                if *self.peek() == TokenKind::LParen {
                    self.advance(); // consume '('
                    let args = self.parse_args()?;
                    self.expect(&TokenKind::RParen)?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::Expr(Expr::Call { name, args }));
                }
                
                // 赋值: ident = expr; 或复合赋值 ident op= expr;
                if *self.peek() == TokenKind::Eq {
                    self.advance(); // consume '='
                    let value = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::Assign { name, value });
                }
                if let Some(op) = Self::compound_binop(self.peek()) {
                    self.advance();
                    let rhs = self.parse_expr()?;
                    self.expect(&TokenKind::Semi)?;
                    return Ok(Statement::Assign {
                        name: name.clone(),
                        value: Expr::BinOp { op, left: Box::new(Expr::Ident(name)), right: Box::new(rhs) },
                    });
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

    /// Phase v1.5: 已消费标识符后继续解析语句（用于 async 作为变量名的情况）
    fn parse_statement_after_ident(&mut self, name: String) -> Result<Statement, String> {
        // PostIncrement: name++
        if *self.peek() == TokenKind::PlusPlus {
            self.advance();
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::PostInc(name));
        }
        // PostDecrement: name--
        if *self.peek() == TokenKind::MinusMinus {
            self.advance();
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::PostDec(name));
        }
        // 函数调用: ident(...)
        if *self.peek() == TokenKind::LParen {
            self.advance();
            let args = self.parse_args()?;
            self.expect(&TokenKind::RParen)?;
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::Expr(Expr::Call { name, args }));
        }
        // 赋值: ident = expr; 或复合赋值 ident op= expr;
        if *self.peek() == TokenKind::Eq {
            self.advance();
            let value = self.parse_expr()?;
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::Assign { name, value });
        }
        if let Some(op) = Self::compound_binop(self.peek()) {
            self.advance();
            let rhs = self.parse_expr()?;
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::Assign {
                name: name.clone(),
                value: Expr::BinOp { op, left: Box::new(Expr::Ident(name)), right: Box::new(rhs) },
            });
        }
        // 表达式语句
        let mut expr = Expr::Ident(name);
        expr = self.parse_expr_continuation(expr)?;
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::Expr(expr))
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

        // Phase A.6: 解构赋值 let { a, b } = obj;
        if *self.peek() == TokenKind::LBrace {
            self.advance(); // consume '{'
            let mut names = Vec::new();
            loop {
                match self.peek().clone() {
                    TokenKind::Ident(n) => { self.advance(); names.push(n); }
                    _ => {
                        let tok = &self.tokens[self.pos];
                        return Err(format!("行{} 列{}: 解构赋值期望标识符", tok.line, tok.col));
                    }
                }
                if *self.peek() == TokenKind::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::RBrace)?;
            self.expect(&TokenKind::Eq)?;
            let source = self.parse_expr()?;
            self.expect(&TokenKind::Semi)?;
            return Ok(Statement::DestructLet { names, source });
        }

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

    /// const name = value; (Phase A.5)
    fn parse_const(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'const'
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => { self.advance(); n }
            _ => {
                let tok = &self.tokens[self.pos];
                return Err(format!("行{} 列{}: const 后期望标识符", tok.line, tok.col));
            }
        };
        self.expect(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::Const { name, value })
    }

    /// switch (expr) { case val: ... default: ... } (Phase A.3)
    fn parse_switch(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'switch'
        self.expect(&TokenKind::LParen)?;
        let discriminant = self.parse_expr()?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::LBrace)?;

        let mut cases = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            match self.peek() {
                TokenKind::Case => {
                    self.advance(); // consume 'case'
                    let test = self.parse_expr()?;
                    self.expect(&TokenKind::Colon)?;
                    let mut body = Vec::new();
                    while *self.peek() != TokenKind::Case
                        && *self.peek() != TokenKind::Default
                        && *self.peek() != TokenKind::RBrace
                        && *self.peek() != TokenKind::Eof
                    {
                        let line = self.tokens[self.pos].line;
                        let stmt = self.parse_statement()?;
                        body.push(SourceStatement { stmt, line });
                    }
                    cases.push(SwitchCase { test: Some(test), body });
                }
                TokenKind::Default => {
                    self.advance(); // consume 'default'
                    self.expect(&TokenKind::Colon)?;
                    let mut body = Vec::new();
                    while *self.peek() != TokenKind::Case
                        && *self.peek() != TokenKind::Default
                        && *self.peek() != TokenKind::RBrace
                        && *self.peek() != TokenKind::Eof
                    {
                        let line = self.tokens[self.pos].line;
                        let stmt = self.parse_statement()?;
                        body.push(SourceStatement { stmt, line });
                    }
                    cases.push(SwitchCase { test: None, body });
                }
                _ => {
                    let tok = &self.tokens[self.pos];
                    return Err(format!("行{} 列{}: switch 中期望 case/default", tok.line, tok.col));
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Statement::Switch { discriminant, cases })
    }

    /// import { items } from "module"; (Phase A.4)
    fn parse_import(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'import'
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        loop {
            match self.peek().clone() {
                TokenKind::Ident(n) => { self.advance(); items.push(n); }
                _ => {
                    let tok = &self.tokens[self.pos];
                    return Err(format!("行{} 列{}: import 期望标识符", tok.line, tok.col));
                }
            }
            if *self.peek() == TokenKind::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        self.expect(&TokenKind::From)?;
        let source = match self.peek().clone() {
            TokenKind::StringLit(s) => { self.advance(); s }
            _ => {
                let tok = &self.tokens[self.pos];
                return Err(format!("行{} 列{}: from 后期望字符串", tok.line, tok.col));
            }
        };
        self.expect(&TokenKind::Semi)?;
        Ok(Statement::Import { items, source })
    }

    /// export function/const/let (Phase A.4)
    fn parse_export(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'export'
        // export function ...
        if *self.peek() == TokenKind::Function {
            let func = self.parse_function()?;
            let _line = 0; // simplified
            // Wrap as export statement: export the function definition isn't directly a Statement
            // We'll just mark it: return Export with inner Let as placeholder
            // Actually, let's emit as Expr for now since functions are tracked separately
            return Ok(Statement::Export {
                item: Box::new(Statement::Let {
                    name: func.name.clone(),
                    value: Expr::Ident(func.name.clone()),
                }),
            });
        }
        // export const/let ...
        let inner = self.parse_statement()?;
        Ok(Statement::Export { item: Box::new(inner) })
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


    fn parse_for(&mut self) -> Result<Statement, String> {
        self.advance(); // consume 'for'
        self.expect(&TokenKind::LParen)?;
        
        // init: let i = expr; or i = expr;
        let init = self.parse_statement()?;
        
        // condition: expr;
        let condition = self.parse_expr()?;
        self.expect(&TokenKind::Semi)?;
        
        // update: i++ or i-- or i = expr (no trailing semicolon)
        let update = self.parse_for_update()?;
        
        self.expect(&TokenKind::RParen)?;
        
        // body
        let body = self.parse_block()?;
        
        Ok(Statement::For {
            init: Box::new(init),
            condition,
            update: Box::new(update),
            body,
        })
    }

    /// Parse for-loop update clause (no trailing semicolon)
    fn parse_for_update(&mut self) -> Result<Statement, String> {
        let name = match self.peek().clone() {
            TokenKind::Ident(n) => n,
            _ => return Err("for update expects identifier".to_string()),
        };
        self.advance(); // consume ident
        
        match self.peek() {
            TokenKind::PlusPlus => { self.advance(); Ok(Statement::PostInc(name)) }
            TokenKind::MinusMinus => { self.advance(); Ok(Statement::PostDec(name)) }
            TokenKind::Eq => {
                self.advance();
                let value = self.parse_expr()?;
                Ok(Statement::Assign { name, value })
            }
            _ if Self::compound_binop(self.peek()).is_some() => {
                let op = Self::compound_binop(self.peek()).unwrap();
                self.advance();
                let rhs = self.parse_expr()?;
                Ok(Statement::Assign {
                    name: name.clone(),
                    value: Expr::BinOp { op, left: Box::new(Expr::Ident(name)), right: Box::new(rhs) },
                })
            }
            _ => {
                let tok = &self.tokens[self.pos];
                Err(format!("line {} col {}: for update expects ++/--/= or += -= *= /= %=", tok.line, tok.col))
            }
        }
    }

    fn parse_block(&mut self) -> Result<Vec<SourceStatement>, String> {
        self.expect(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while *self.peek() != TokenKind::RBrace && *self.peek() != TokenKind::Eof {
            let line = self.tokens[self.pos].line;
            let stmt = self.parse_statement()?;
            stmts.push(SourceStatement { stmt, line });
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(stmts)
    }

    /// 表达式解析（运算符优先级）
    fn parse_expr(&mut self) -> Result<Expr, String> {
        let expr = self.parse_logical_or()?;
        // Phase A.7: 三元运算符 cond ? a : b
        if *self.peek() == TokenKind::Question {
            self.advance(); // consume '?'
            let then_expr = self.parse_expr()?;
            self.expect(&TokenKind::Colon)?;
            let else_expr = self.parse_expr()?;
            return Ok(Expr::Ternary {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(expr)
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
        let mut left = self.parse_bitwise_or()?;
        while *self.peek() == TokenKind::AmpAmp {
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = Expr::BinOp { op: BinOp::And, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    // 位运算债清偿: | ^ & 三层优先级（JS 语义: & > ^ > | > 逻辑运算）
    fn parse_bitwise_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_xor()?;
        while *self.peek() == TokenKind::Pipe {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = Expr::BinOp { op: BinOp::BitOr, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bitwise_and()?;
        while *self.peek() == TokenKind::Caret {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = Expr::BinOp { op: BinOp::BitXor, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_equality()?;
        while *self.peek() == TokenKind::Amp {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::BinOp { op: BinOp::BitAnd, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// 相等性比较（== != === !==），介于按位与和关系比较之间
    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek() {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::EqEqEq => BinOp::StrictEq,
                TokenKind::NotEq => BinOp::NotEq,
                TokenKind::NotEqEq => BinOp::StrictNotEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Ge => BinOp::Ge,
                TokenKind::Gt => BinOp::Gt,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::BinOp { op, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    /// 位运算债清偿: 移位层 << >> >>>（介于关系比较和加减之间）
    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                TokenKind::Shrrr => BinOp::UShr,
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
        // 一元运算符脱糖（复用现有 BinOp 节点，零后端改动）：
        //   -x → (0 - x)   +x → x   !x → (x == 0)
        match self.peek() {
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::BinOp { op: BinOp::Sub, left: Box::new(Expr::Number(0)), right: Box::new(operand) })
            }
            TokenKind::Plus => {
                self.advance();
                self.parse_unary()
            }
            TokenKind::Bang => {
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::BinOp { op: BinOp::Eq, left: Box::new(operand), right: Box::new(Expr::Number(0)) })
            }
            TokenKind::Tilde => {
                // 位运算债清偿: ~x 脱糖为 (-1 - x)，i64 下与按位取反等价，零后端改动
                self.advance();
                let operand = self.parse_unary()?;
                Ok(Expr::BinOp { op: BinOp::Sub, left: Box::new(Expr::Number(-1)), right: Box::new(operand) })
            }
            _ => {
                let primary = self.parse_primary()?;
                self.parse_postfix(primary)
            }
        }
    }

    /// 复合赋值 token → BinOp（供脱糖为 Assign/IndexAssign/PropAssign + BinOp）
    fn compound_binop(kind: &TokenKind) -> Option<BinOp> {
        match kind {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Mod),
            _ => None,
        }
    }

    /// 向前看检查是否是箭头函数: (...)=>
    fn is_arrow_function(&self) -> bool {
        // 找到匹配的右括号，然后检查后面是否有 =>
        let mut depth = 0;
        let mut i = self.pos;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LParen => { depth += 1; }
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        // 检查下一个 token 是否是 =>
                        return i + 1 < self.tokens.len()
                            && self.tokens[i + 1].kind == TokenKind::Arrow;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// 解析箭头函数 (params) => expr 或 (params) => { body }
    fn parse_arrow_function(&mut self) -> Result<Expr, String> {
        self.advance(); // consume '('
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Arrow)?;

        // 块体或表达式体
        let body = if *self.peek() == TokenKind::LBrace {
            let stmts = self.parse_block()?;
            ArrowBody::Block(stmts)
        } else {
            let expr = self.parse_expr()?;
            ArrowBody::Expr(Box::new(expr))
        };

        Ok(Expr::ArrowFunc { params, body, is_async: false })
    }

    /// 后缀操作符: arr[i] + obj.key (Phase 10) + str.method() (Phase A.8)
    fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            match self.peek() {
                TokenKind::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(&TokenKind::RBracket)?;
                    expr = Expr::Index { array: Box::new(expr), index: Box::new(index) };
                }
                TokenKind::Dot => {
                    self.advance();
                    let prop = match self.peek().clone() {
                        TokenKind::Ident(p) => { self.advance(); p }
                        _ => return Err("property: . expects ident".to_string()),
                    };
                    // Phase A.8: 字符串方法 .length / .charAt(i) / .slice(a, b)
                    // 语言完备性批 2: .startsWith/.endsWith/.includes/.charCodeAt
                    if prop == "length" || prop == "charAt" || prop == "slice" || prop == "substring" || prop == "indexOf"
                        || prop == "startsWith" || prop == "endsWith" || prop == "includes" || prop == "charCodeAt" {
                        if *self.peek() == TokenKind::LParen {
                            self.advance(); // consume '('
                            let args = self.parse_args()?;
                            self.expect(&TokenKind::RParen)?;
                            expr = Expr::StringMethod {
                                object: Box::new(expr),
                                method: prop,
                                args,
                            };
                        } else if prop == "length" {
                            // .length 无括号
                            expr = Expr::StringMethod {
                                object: Box::new(expr),
                                method: prop,
                                args: vec![],
                            };
                        } else {
                            expr = Expr::PropAccess { object: Box::new(expr), property: prop };
                        }
                    } else if *self.peek() == TokenKind::LParen {
                        // Phase C.3: 对象方法调用 obj.method(args)
                        // 生成 Call { name: "obj.method", args }
                        self.advance(); // consume '('
                        let call_name = match &expr {
                            Expr::Ident(obj_name) => format!("{}.{}", obj_name, prop),
                            _ => prop.clone(),
                        };
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        expr = Expr::Call { name: call_name, args };
                    } else {
                        expr = Expr::PropAccess { object: Box::new(expr), property: prop };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            TokenKind::Number(n) => { self.advance(); Ok(Expr::Number(n)) }
            TokenKind::StringLit(s) => { self.advance(); Ok(Expr::StringLit(s)) }
            // Phase A: true / false / null
            TokenKind::True => { self.advance(); Ok(Expr::Bool(true)) }
            TokenKind::False => { self.advance(); Ok(Expr::Bool(false)) }
            TokenKind::Null => { self.advance(); Ok(Expr::Null) }
            // Phase v1.5: await expr
            TokenKind::Await => {
                self.advance(); // consume 'await'
                let expr = self.parse_unary()?;
                Ok(Expr::Await { expr: Box::new(expr) })
            }
            // Phase v1.5: async 箭头函数 async (params) => ...
            TokenKind::Async => {
                self.advance(); // consume 'async'
                if *self.peek() == TokenKind::LParen && self.is_arrow_function() {
                    // async (params) => body
                    self.advance(); // consume '('
                    let params = self.parse_params()?;
                    self.expect(&TokenKind::RParen)?;
                    self.expect(&TokenKind::Arrow)?;
                    let body = if *self.peek() == TokenKind::LBrace {
                        let stmts = self.parse_block()?;
                        ArrowBody::Block(stmts)
                    } else {
                        let expr = self.parse_expr()?;
                        ArrowBody::Expr(Box::new(expr))
                    };
                    Ok(Expr::ArrowFunc { params, body, is_async: true })
                } else {
                    // async 作为标识符
                    let name = "async".to_string();
                    if *self.peek() == TokenKind::LParen {
                        self.advance();
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        Ok(Expr::Call { name, args })
                    } else {
                        Ok(Expr::Ident(name))
                    }
                }
            }
            // Phase v1.5: Promise.all([...]) / Promise.race([...])
            TokenKind::Ident(ref name) if name == "Promise" => {
                self.advance(); // consume 'Promise'
                self.expect(&TokenKind::Dot)?;
                let method = match self.peek().clone() {
                    TokenKind::Ident(ref m) => m.clone(),
                    _ => return Err("Promise 后期望 .all/.race".to_string()),
                };
                self.advance(); // consume method name
                self.expect(&TokenKind::LParen)?;
                self.expect(&TokenKind::LBracket)?;
                let mut promises = Vec::new();
                if *self.peek() != TokenKind::RBracket {
                    loop {
                        promises.push(self.parse_expr()?);
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                self.expect(&TokenKind::RParen)?;
                match method.as_str() {
                    "all" => Ok(Expr::PromiseAll { promises }),
                    "race" => Ok(Expr::PromiseRace { promises }),
                    _ => Err(format!("不支持的 Promise.{}", method)),
                }
            }
            // Phase A.2: 模板字符串
            TokenKind::TemplateLit(ref parts) => {
                let parts = parts.clone();
                self.advance();
                let expr_parts: Vec<TemplateExprPart> = parts.into_iter().map(|p| match p {
                    TemplatePart::Text(t) => TemplateExprPart::Text(t),
                    TemplatePart::Expr(e) => {
                        // 递归解析内嵌表达式
                        let tokens = crate::lexer::tokenize(&e).unwrap_or_default();
                        if tokens.is_empty() {
                            TemplateExprPart::Text(e)
                        } else {
                            let mut inner_parser = Parser::new(&tokens);
                            match inner_parser.parse_expr() {
                                Ok(expr) => TemplateExprPart::Expr(expr),
                                Err(_) => TemplateExprPart::Text(e),
                            }
                        }
                    }
                }).collect();
                Ok(Expr::TemplateStr { parts: expr_parts })
            }
            TokenKind::Ident(ref name) if name == "Math" => {
                self.advance(); // consume 'Math'
                self.expect(&TokenKind::Dot)?;
                let method = match self.peek().clone() {
                    TokenKind::Ident(ref m) => m.clone(),
                    _ => return Err("Math 后期望 .max/.min/.abs/.sqrt/.pow".to_string()),
                };
                self.advance(); // consume method name
                // 语言完备性批 2：整数语义下的脱糖（本语言所有值均为整数）
                //   floor/trunc/round/ceil(x) ≡ x（整数的取整/取顶是恒等）
                //   sign(x) ≡ (x > 0) - (x < 0)
                match method.as_str() {
                    "floor" | "trunc" | "round" | "ceil" => {
                        self.expect(&TokenKind::LParen)?;
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        if args.len() != 1 {
                            return Err(format!("Math.{} 需要 1 个参数", method));
                        }
                        return Ok(args.into_iter().next().unwrap());
                    }
                    "sign" => {
                        self.expect(&TokenKind::LParen)?;
                        let args = self.parse_args()?;
                        self.expect(&TokenKind::RParen)?;
                        if args.len() != 1 {
                            return Err("Math.sign 需要 1 个参数".to_string());
                        }
                        let x = args.into_iter().next().unwrap();
                        return Ok(Expr::BinOp {
                            op: BinOp::Sub,
                            left: Box::new(Expr::BinOp { op: BinOp::Gt, left: Box::new(x.clone()), right: Box::new(Expr::Number(0)) }),
                            right: Box::new(Expr::BinOp { op: BinOp::Lt, left: Box::new(x), right: Box::new(Expr::Number(0)) }),
                        });
                    }
                    _ => {}
                }
                let op = match method.as_str() {
                    "max" => MathOp::Max,
                    "min" => MathOp::Min,
                    "abs" => MathOp::Abs,
                    "sqrt" => MathOp::Sqrt,
                    "pow" => MathOp::Pow,
                    _ => return Err(format!("不支持的 Math.{}", method)),
                };
                self.expect(&TokenKind::LParen)?;
                let args = self.parse_args()?;
                self.expect(&TokenKind::RParen)?;
                // 参数数量前置检查：避免空参数在下游后端越界
                match op {
                    MathOp::Abs | MathOp::Sqrt => {
                        if args.len() != 1 { return Err(format!("Math.{} 需要 1 个参数", method)); }
                    }
                    MathOp::Max | MathOp::Min | MathOp::Pow => {
                        if args.len() != 2 { return Err(format!("Math.{} 需要 2 个参数", method)); }
                    }
                }
                Ok(Expr::MathIntrinsic { op, args })
            }
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
                // Phase A.1: 可能是箭头函数 (params) => ...
                // 或普通括号表达式 (expr)
                // 向前看: 检查是否是箭头函数
                if self.is_arrow_function() {
                    return self.parse_arrow_function();
                }
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
            // Phase 10: 对象字面量 { key: value, ... }
            TokenKind::LBrace => {
                self.advance(); // consume '{'
                let mut keys = Vec::new();
                let mut values = Vec::new();
                if *self.peek() != TokenKind::RBrace {
                    loop {
                        let key = match self.peek().clone() {
                            TokenKind::Ident(k) => { self.advance(); k }
                            TokenKind::StringLit(k) => { self.advance(); k }
                            _ => return Err("object: expects property name".to_string()),
                        };
                        self.expect(&TokenKind::Colon)?;
                        let value = self.parse_expr()?;
                        keys.push(key);
                        values.push(value);
                        if *self.peek() == TokenKind::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr::ObjectLit { keys, values })
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

    // 语言完备性批 1：一元运算脱糖 + 复合赋值脱糖
    #[test]
    fn test_parse_unary_minus() {
        let tokens = tokenize("let x = -5;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Sub,
                    left: Box::new(Expr::Number(0)),
                    right: Box::new(Expr::Number(5)),
                });
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_unary_plus_is_identity() {
        let tokens = tokenize("let x = +3;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => assert_eq!(*value, Expr::Number(3)),
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_unary_not() {
        let tokens = tokenize("let f = !x;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Ident("x".to_string())),
                    right: Box::new(Expr::Number(0)),
                });
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_unary_binding() {
        // -x * y → (0 - x) * y，一元优先级高于乘法
        let tokens = tokenize("let z = -x * y;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Mul,
                    left: Box::new(Expr::BinOp {
                        op: BinOp::Sub,
                        left: Box::new(Expr::Number(0)),
                        right: Box::new(Expr::Ident("x".to_string())),
                    }),
                    right: Box::new(Expr::Ident("y".to_string())),
                });
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    #[test]
    fn test_parse_compound_assign_simple() {
        let tokens = tokenize("x += 2;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Assign { name, value } => {
                assert_eq!(name, "x");
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Add,
                    left: Box::new(Expr::Ident("x".to_string())),
                    right: Box::new(Expr::Number(2)),
                });
            }
            _ => panic!("期望 Assign 语句"),
        }
    }

    #[test]
    fn test_parse_compound_assign_index() {
        let tokens = tokenize("arr[i] %= 3;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::IndexAssign { name, index, value } => {
                assert_eq!(name, "arr");
                assert_eq!(*index, Expr::Ident("i".to_string()));
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Mod,
                    left: Box::new(Expr::Index {
                        array: Box::new(Expr::Ident("arr".to_string())),
                        index: Box::new(Expr::Ident("i".to_string())),
                    }),
                    right: Box::new(Expr::Number(3)),
                });
            }
            _ => panic!("期望 IndexAssign 语句"),
        }
    }

    #[test]
    fn test_parse_compound_assign_prop() {
        let tokens = tokenize("obj.count -= 1;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::PropAssign { object, property, value } => {
                assert_eq!(object, "obj");
                assert_eq!(property, "count");
                assert_eq!(*value, Expr::BinOp {
                    op: BinOp::Sub,
                    left: Box::new(Expr::PropAccess {
                        object: Box::new(Expr::Ident("obj".to_string())),
                        property: "count".to_string(),
                    }),
                    right: Box::new(Expr::Number(1)),
                });
            }
            _ => panic!("期望 PropAssign 语句"),
        }
    }

    #[test]
    fn test_parse_for_update_compound() {
        let tokens = tokenize("for (let i = 0; i < 3; i += 1) { }").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::For { update, .. } => {
                match &**update {
                    Statement::Assign { name, value } => {
                        assert_eq!(name, "i");
                        assert_eq!(*value, Expr::BinOp {
                            op: BinOp::Add,
                            left: Box::new(Expr::Ident("i".to_string())),
                            right: Box::new(Expr::Number(1)),
                        });
                    }
                    _ => panic!("期望 Assign 更新子句"),
                }
            }
            _ => panic!("期望 For 语句"),
        }
    }

    #[test]
    fn test_parse_let() {
        let tokens = tokenize("let x = 42;").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 1);
        match &program.statements[0].stmt {
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
        assert_eq!(program.functions[0].func.name, "add");
        assert_eq!(program.functions[0].func.params, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_call() {
        let tokens = tokenize("let x = add(3, 4);").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
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
        match &program.statements[0].stmt {
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
        match &program.statements[0].stmt {
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
        match &program.statements[0].stmt {
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
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert!(matches!(value, Expr::Index { .. }));
            }
            _ => panic!("期望 Let 语句"),
        }
    }

    // Phase A 测试

    #[test]
    fn test_parse_const() {
        let tokens = tokenize("const MAX = 100;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Const { name, value } => {
                assert_eq!(name, "MAX");
                assert_eq!(*value, Expr::Number(100));
            }
            _ => panic!("期望 Const 语句"),
        }
    }

    #[test]
    fn test_parse_bool_null() {
        let tokens = tokenize("let a = true; let b = false; let c = null;").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.statements.len(), 3);
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => assert_eq!(*value, Expr::Bool(true)),
            _ => panic!("期望 Let"),
        }
        match &program.statements[1].stmt {
            Statement::Let { value, .. } => assert_eq!(*value, Expr::Bool(false)),
            _ => panic!("期望 Let"),
        }
        match &program.statements[2].stmt {
            Statement::Let { value, .. } => assert_eq!(*value, Expr::Null),
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_ternary() {
        let tokens = tokenize("let x = a > b ? 1 : 0;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert!(matches!(value, Expr::Ternary { .. }));
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_switch() {
        let source = "switch (x) { case 1: console.log(1); break; case 2: console.log(2); break; default: console.log(0); break; }";
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Switch { cases, .. } => {
                assert_eq!(cases.len(), 3); // 2 cases + 1 default
                assert!(cases[0].test.is_some());
                assert!(cases[2].test.is_none()); // default
            }
            _ => panic!("期望 Switch"),
        }
    }

    #[test]
    fn test_parse_arrow_func_expr() {
        let tokens = tokenize("let add = (a, b) => a + b;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::ArrowFunc { params, body, .. } => {
                        assert_eq!(params, &vec!["a".to_string(), "b".to_string()]);
                        assert!(matches!(body, ArrowBody::Expr(_)));
                    }
                    _ => panic!("期望 ArrowFunc"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_arrow_block() {
        let tokens = tokenize("let fn = (n) => { return n * n; };").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::ArrowFunc { params, body, .. } => {
                        assert_eq!(params.len(), 1);
                        assert!(matches!(body, ArrowBody::Block(_)));
                    }
                    _ => panic!("期望 ArrowFunc"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_template_string() {
        let tokens = tokenize("let msg = `hello`;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert!(matches!(value, Expr::TemplateStr { .. }));
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_strict_eq() {
        let tokens = tokenize("let x = a === b;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::BinOp { op, .. } => assert_eq!(*op, BinOp::StrictEq),
                    _ => panic!("期望 BinOp"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_import() {
        let tokens = tokenize("import { GPIO, UART } from \"hardy:hw\";").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Import { items, source } => {
                assert_eq!(items, &vec!["GPIO".to_string(), "UART".to_string()]);
                assert_eq!(source, "hardy:hw");
            }
            _ => panic!("期望 Import"),
        }
    }

    #[test]
    fn test_parse_destruct_let() {
        let tokens = tokenize("let { x, y } = point;").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::DestructLet { names, source } => {
                assert_eq!(names, &vec!["x".to_string(), "y".to_string()]);
                assert_eq!(*source, Expr::Ident("point".to_string()));
            }
            _ => panic!("期望 DestructLet"),
        }
    }

    // Phase v1.5: async/await 语法测试
    #[test]
    fn test_parse_async_function() {
        let tokens = tokenize("async function readSensor() { return 1; }").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.functions.len(), 1);
        let func = &program.functions[0].func;
        assert_eq!(func.name, "readSensor");
        assert!(func.is_async);
    }

    #[test]
    fn test_parse_await_expr() {
        let tokens = tokenize("let x = await sensor.read();").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::Await { expr } => {
                        match expr.as_ref() {
                            Expr::Call { name, .. } => assert_eq!(name, "sensor.read"),
                            _ => panic!("期望 Call"),
                        }
                    }
                    _ => panic!("期望 Await"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_promise_all() {
        let tokens = tokenize("let r = Promise.all([a(), b()]);").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::PromiseAll { promises } => {
                        assert_eq!(promises.len(), 2);
                    }
                    _ => panic!("期望 PromiseAll"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_promise_race() {
        let tokens = tokenize("let r = Promise.race([x(), y()]);").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::PromiseRace { promises } => {
                        assert_eq!(promises.len(), 2);
                    }
                    _ => panic!("期望 PromiseRace"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_parse_async_arrow() {
        let tokens = tokenize("let fn = async () => { return 1; };").unwrap();
        let program = parse(&tokens).unwrap();
        match &program.statements[0].stmt {
            Statement::Let { value, .. } => {
                match value {
                    Expr::ArrowFunc { is_async, .. } => {
                        assert!(*is_async);
                    }
                    _ => panic!("期望 ArrowFunc"),
                }
            }
            _ => panic!("期望 Let"),
        }
    }

    // Phase 3.1: @realtime 注解解析测试
    #[test]
    fn test_parse_realtime_annotation() {
        let tokens = tokenize("@realtime\nfunction motorCtrl() { let x = 1; }").unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.functions.len(), 1);
        let func = &program.functions[0].func;
        assert!(func.is_realtime, "应标记为 realtime");
        assert_eq!(func.wcet_limit_us, None, "无参数时 WCET 为 None");
        assert_eq!(func.name, "motorCtrl");
    }

    #[test]
    fn test_parse_realtime_with_wcet() {
        let tokens = tokenize("@realtime(1000)\nfunction readEncoder() { return 0; }").unwrap();
        let program = parse(&tokens).unwrap();
        let func = &program.functions[0].func;
        assert!(func.is_realtime);
        assert_eq!(func.wcet_limit_us, Some(1000), "WCET 应为 1000us");
    }

    #[test]
    fn test_parse_interrupt_annotation() {
        let src = "@interrupt(\"TIM2\", period_us: 100)\nfunction controlLoop() { let x = 1; }";
        let tokens = tokenize(src).unwrap();
        let program = parse(&tokens).unwrap();
        let func = &program.functions[0].func;
        assert_eq!(func.interrupt_timer, Some("TIM2".to_string()));
        assert_eq!(func.interrupt_period_us, Some(100));
    }

    #[test]
    fn test_parse_priority_annotation() {
        let tokens = tokenize("@priority(0)\n@realtime\nfunction highest() { let x = 1; }").unwrap();
        let program = parse(&tokens).unwrap();
        let func = &program.functions[0].func;
        assert_eq!(func.priority, Some(0), "优先级应为 0");
        assert!(func.is_realtime, "应标记为 realtime");
    }

    #[test]
    fn test_parse_realtime_async() {
        let tokens = tokenize("@realtime\nasync function rt_async() { let x = 1; }").unwrap();
        let program = parse(&tokens).unwrap();
        let func = &program.functions[0].func;
        assert!(func.is_realtime);
        assert!(func.is_async, "应标记为 async");
    }
}
