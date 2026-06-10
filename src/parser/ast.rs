//! AST 节点定义
//! Phase 6: 添加源码行号跟踪

/// 程序（顶层）
#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<SourceStatement>,
    pub functions: Vec<SourceFunction>,
}

/// 带源码行号的语句
#[derive(Debug, Clone, PartialEq)]
pub struct SourceStatement {
    pub stmt: Statement,
    pub line: usize,
}

/// 带源码行号的函数
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFunction {
    pub func: Function,
    pub line: usize,
}

/// 函数定义
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<SourceStatement>,
}

/// 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// let name = value;
    Let { name: String, value: Expr },
    /// name = value;
    Assign { name: String, value: Expr },
    /// name[index] = value;
    IndexAssign { name: String, index: Expr, value: Expr },
    /// return expr;
    Return(Expr),
    /// if (cond) { ... } else { ... }
    If {
        condition: Expr,
        then_body: Vec<SourceStatement>,
        else_body: Option<Vec<SourceStatement>>,
    },
    /// while (cond) { ... }
    While {
        condition: Expr,
        body: Vec<SourceStatement>,
    },
    /// for (init; cond; update) { body }
    For {
        init: Box<Statement>,
        condition: Expr,
        update: Box<Statement>,
        body: Vec<SourceStatement>,
    },
    /// name++ (Phase 9)
    PostInc(String),
    /// name-- (Phase 9)
    PostDec(String),
    /// console.log(args...)
    ConsoleLog(Vec<Expr>),
    /// 表达式语句
    Expr(Expr),
}

/// 表达式
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(i64),
    StringLit(String),
    Ident(String),
    BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    Array(Vec<Expr>),
    Index { array: Box<Expr>, index: Box<Expr> },
    /// Math.max/min/abs 内建 → COUNPRE64 创新指令
    MathIntrinsic { op: MathOp, args: Vec<Expr> },
}

/// 二元运算符
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, NotEq, Lt, Gt, Le, Ge,
    And, Or,
}

/// Math 内建运算 → COUNPRE64 创新指令 (单周期)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathOp {
    Max,  // opcode=0x0, func=0xB
    Min,  // opcode=0x0, func=0xC
    Abs,  // opcode=0x0, func=0xD (单参数, rs2=0)
}
