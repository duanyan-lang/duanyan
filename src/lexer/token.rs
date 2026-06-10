//! Token 定义

/// Token 类型
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // 字面量
    Number(i64),
    StringLit(String),  // Phase 4: 字符串字面量
    Ident(String),

    // 关键字
    Let,
    Function,
    If,
    Else,
    While,
    For,
    Return,

    // 运算符
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Eq,         // =
    EqEq,       // ==
    NotEq,      // !=
    Bang,       // !
    Lt,         // <
    Gt,         // >
    Percent,    // %  Phase 4: 取模
    AmpAmp,     // && Phase 4: 逻辑与
    PipePipe,   // || Phase 4: 逻辑或

    // 分隔符
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [  Phase 4: 数组
    RBracket,   // ]  Phase 4: 数组
    Semi,       // ;
    Comma,      // ,
    Dot,        // .  Phase 4: 成员访问
    PlusPlus,   // ++ Phase 9: for循环
    MinusMinus, // -- Phase 9: for循环
    Le,         // <= Phase 9
    Ge,         // >= Phase 9

    // 结束
    Eof,
}

/// Token 结构（含位置信息）
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}
