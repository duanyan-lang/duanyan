//! Token 定义

/// Token 类型
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // 字面量
    Number(i64),
    StringLit(String),  // Phase 4: 字符串字面量
    TemplateLit(Vec<TemplatePart>),  // Phase A.2: 模板字符串
    Ident(String),

    // 关键字
    Let,
    Const,       // Phase A.5
    Function,
    If,
    Else,
    While,
    For,
    Return,
    Switch,      // Phase A.3
    Case,        // Phase A.3
    Default,     // Phase A.3
    Break,       // Phase A.3
    Continue,    // 语言完备性批 2
    Import,      // Phase A.4
    Export,      // Phase A.4
    From,        // Phase A.4
    True,        // Phase A
    False,       // Phase A
    Null,        // Phase A
    Async,       // Phase v1.5: async
    Await,       // Phase v1.5: await

    // 运算符
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Eq,         // =
    EqEq,       // ==
    EqEqEq,     // === Phase A
    NotEq,      // !=
    NotEqEq,    // !== Phase A
    Bang,       // !
    Lt,         // <
    Gt,         // >
    Percent,    // %  Phase 4: 取模
    AmpAmp,     // && Phase 4: 逻辑与
    PipePipe,   // || Phase 4: 逻辑或
    Arrow,      // => Phase A.1: 箭头函数
    Question,   // ?  Phase A.7: 三元运算符

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
    PlusEq,     // += 复合赋值
    MinusEq,    // -= 复合赋值
    StarEq,     // *= 复合赋值
    SlashEq,    // /= 复合赋值
    PercentEq,  // %= 复合赋值
    Le,         // <= Phase 9
    Ge,         // >= Phase 9
    Colon,      // :  Phase 10: 对象属性
    At,         // @  Phase 3: 注解 (@realtime, @interrupt, @priority)

    // 结束
    Eof,
}

/// 模板字符串的一部分
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    /// 纯文本
    Text(String),
    /// 插值表达式 `${expr}`
    Expr(String),
}

/// Token 结构（含位置信息）
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}
