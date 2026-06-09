//! AST 节点定义
//! Phase 4: 支持数组、字符串、console.log

/// 程序（顶层）
#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
    pub functions: Vec<Function>,
}

/// 函数定义
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Statement>,
}

/// 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// let name = value;
    Let { name: String, value: Expr },
    /// name = value;
    Assign { name: String, value: Expr },
    /// name[index] = value;  (Phase 4)
    IndexAssign { name: String, index: Expr, value: Expr },
    /// return expr;
    Return(Expr),
    /// if (cond) { ... } else { ... }
    If {
        condition: Expr,
        then_body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    /// while (cond) { ... }
    While {
        condition: Expr,
        body: Vec<Statement>,
    },
    /// console.log(args...)  (Phase 4)
    ConsoleLog(Vec<Expr>),
    /// 表达式语句
    Expr(Expr),
}

/// 表达式
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 整数字面量
    Number(i64),
    /// 字符串字面量 (Phase 4)
    StringLit(String),
    /// 标识符（变量名）
    Ident(String),
    /// 二元运算
    BinOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// 函数调用
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// 数组字面量 (Phase 4)
    Array(Vec<Expr>),
    /// 数组索引 (Phase 4)
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
    },
}

/// 二元运算符
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,    // +
    Sub,    // -
    Mul,    // *
    Div,    // /
    Mod,    // %  Phase 4
    Eq,     // ==
    NotEq,  // !=
    Lt,     // <
    Gt,     // >
    And,    // && Phase 4
    Or,     // || Phase 4
}
