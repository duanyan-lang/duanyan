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
    pub is_async: bool,  // Phase v1.5: async function
    // Phase 3.1: 实时注解
    pub is_realtime: bool,
    pub wcet_limit_us: Option<u32>,
    pub interrupt_timer: Option<String>,    // @interrupt("TIM2", ...)
    pub interrupt_period_us: Option<u32>,   // @interrupt(..., period_us: 100)
    pub priority: Option<u8>,               // @priority(0)
}

/// 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// let name = value;
    Let { name: String, value: Expr },
    /// const name = value; (Phase A.5)
    Const { name: String, value: Expr },
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
    /// switch (expr) { case val: ... default: ... } (Phase A.3)
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// break; (Phase A.3)
    Break,
    /// continue; (语言完备性批 2)
    Continue,
    /// name++ (Phase 9)
    PostInc(String),
    /// name-- (Phase 9)
    PostDec(String),
    /// obj.key = value (Phase 10)
    PropAssign { object: String, property: String, value: Expr },
    /// console.log(args...)
    ConsoleLog(Vec<Expr>),
    /// import { items } from "module" (Phase A.4)
    Import { items: Vec<String>, source: String },
    /// export { items } or export function/const (Phase A.4)
    Export { item: Box<Statement> },
    /// let { a, b } = obj; 解构赋值 (Phase A.6)
    DestructLet { names: Vec<String>, source: Expr },
    /// 表达式语句
    Expr(Expr),
}

/// switch case 分支 (Phase A.3)
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub test: Option<Expr>,  // None = default
    pub body: Vec<SourceStatement>,
}

/// 表达式
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(i64),
    StringLit(String),
    Ident(String),
    Bool(bool),           // Phase A: true/false
    Null,                 // Phase A: null
    BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    Array(Vec<Expr>),
    Index { array: Box<Expr>, index: Box<Expr> },
    /// Math.max/min/abs 内建 → COUNPRE64 创新指令
    MathIntrinsic { op: MathOp, args: Vec<Expr> },
    /// { key: value, ... } 对象字面量 (Phase 10)
    ObjectLit { keys: Vec<String>, values: Vec<Expr> },
    /// obj.key 属性访问 (Phase 10)
    PropAccess { object: Box<Expr>, property: String },
    /// cond ? a : b (Phase A.7)
    Ternary { condition: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr> },
    /// (params) => body 或 (params) => { body } (Phase A.1)
    ArrowFunc { params: Vec<String>, body: ArrowBody, is_async: bool },
    /// await expr (Phase v1.5)
    Await { expr: Box<Expr> },
    /// Promise.all([p1, p2, ...]) (Phase v1.5)
    PromiseAll { promises: Vec<Expr> },
    /// Promise.race([p1, p2, ...]) (Phase v1.5)
    PromiseRace { promises: Vec<Expr> },
    /// `${text}${expr}${text}` (Phase A.2)
    TemplateStr { parts: Vec<TemplateExprPart> },
    /// str.length, str.charAt(i), str.slice(a, b) (Phase A.8)
    StringMethod { object: Box<Expr>, method: String, args: Vec<Expr> },
}

/// 箭头函数体
#[derive(Debug, Clone, PartialEq)]
pub enum ArrowBody {
    /// => expr (单表达式)
    Expr(Box<Expr>),
    /// => { statements } (块)
    Block(Vec<SourceStatement>),
}

/// 模板字符串的表达式部分
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateExprPart {
    Text(String),
    Expr(Expr),
}

/// 二元运算符
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, NotEq, StrictEq, StrictNotEq,  // Phase A: ===, !==
    Lt, Gt, Le, Ge,
    And, Or,
    // 位运算债清偿: & | ^ << >> >>>
    BitAnd, BitOr, BitXor, Shl, Shr, UShr,
}

/// Math 内建运算 → COUNPRE64 创新指令 (单周期)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathOp {
    Max,  // opcode=0x0, func=0xB
    Min,  // opcode=0x0, func=0xC
    Abs,  // opcode=0x0, func=0xD (单参数, rs2=0)
    /// 整数平方根（仅 C 目标：hardy_isqrt helper）
    Sqrt,
    /// 整数幂（仅 C 目标：hardy_ipow helper）
    Pow,
}
