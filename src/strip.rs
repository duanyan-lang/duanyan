//! TypeScript 类型剥离器 (Type Stripper)
//!
//! 将 .ts 文件中的类型注解剥离为纯 JS 子集
//! 策略：编译前预处理，移除所有 TypeScript 特有语法

/// 剥离 TypeScript 类型注解，输出纯 JS
pub fn strip_types(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // 跳过字符串内容
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let quote = chars[i];
            result.push(chars[i]);
            i += 1;
            while i < len && chars[i] != quote {
                if chars[i] == '\\' {
                    result.push(chars[i]);
                    i += 1;
                    if i < len {
                        result.push(chars[i]);
                        i += 1;
                    }
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if i < len {
                result.push(chars[i]); // closing quote
                i += 1;
            }
            continue;
        }

        // 跳过单行注释
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' {
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // 跳过 "interface xxx { ... }" 整块
        if starts_with_word(&chars, i, "interface") {
            skip_until_block_end(&chars, &mut i);
            result.push('\n');
            continue;
        }

        // 跳过 "type xxx = ..." 整行
        if starts_with_word(&chars, i, "type") && is_type_alias(&chars, i) {
            while i < len && chars[i] != ';' && chars[i] != '\n' {
                i += 1;
            }
            if i < len && chars[i] == ';' { i += 1; }
            result.push('\n');
            continue;
        }

        // 跳过 "as Type" 类型断言
        if starts_with_word(&chars, i, "as") {
            let saved = i;
            i += 2;
            skip_whitespace(&chars, &mut i);
            // 跳过类型名
            if i < len && (chars[i].is_alphabetic() || chars[i] == '_') {
                skip_type_expr(&chars, &mut i);
                continue;
            } else {
                i = saved;
            }
        }

        // 参数/变量后的 ": Type" 注解
        if chars[i] == ':' && i > 0 && is_type_annotation_context(&chars, i) {
            i += 1; // skip ':'
            skip_whitespace(&chars, &mut i);
            skip_type_expr(&chars, &mut i);
            // 保留一个空格分隔后续内容(= { 等)
            if i < len && chars[i] != ',' && chars[i] != ')' && chars[i] != ';' && !chars[i].is_whitespace() {
                result.push(' ');
            }
            continue;
        }

        // 泛型参数 "<T>" 或 "<T, U>"（函数名/类型后）
        if chars[i] == '<' && i > 0 && is_generic_context(&chars, i) {
            if let Some(end) = find_generic_end(&chars, i) {
                i = end + 1;
                continue;
            }
        }

        // 普通字符
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// 宽度安全整数类型（扩展 1）
pub const WIDTH_TYPES: &[&str] = &["u8", "u16", "u32", "i8", "i16", "i32"];
/// 定点/浮点类型（扩展 3）
pub const FIXED_TYPES: &[&str] = &["q15", "q16", "f32"];

/// 是否为 DUANYAN 扩展数值类型（宽度/定点/浮点）
pub fn is_ext_numeric_type(t: &str) -> bool {
    WIDTH_TYPES.contains(&t) || FIXED_TYPES.contains(&t)
}

/// f32 被赋予小数字面量的哨兵类型值（H2）：codegen 见此标记 + 数字字面量 → 显式报错。
pub const F32_DECIMAL_SENTINEL: &str = "f32.decimal";

/// 旁路提取宽度/定点注解（扩展 1/3）。
/// 扫描 `ident : T`（T ∈ WIDTH_TYPES ∪ FIXED_TYPES），返回 变量名→类型 表。
/// 不修改源码、不进 AST：类型信息经此表旁路送入 codegen，保持宿主语法冻结。
///
/// H1 保守回退：旁路表按变量名全局生效、无作用域信息。为避免“同名变量一处带注解、
/// 另一处普通声明”导致的静默误截断，若某名字同时存在普通（无扩展注解）声明，则从表中
/// 剔除（回退 int32_t）——宁可丢失宽度也不误截断。
/// H2 标记：DUANYAN 词法层把小数统一 ×100 缩放为整数，f32 变量若被赋予小数字面量则无法
/// 忠实表示真浮点值，此类名字标记为 F32_DECIMAL_SENTINEL，由 codegen 显式报错。
pub fn extract_width_types(source: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        // 跳过字符串，避免误捕
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let q = chars[i];
            i += 1;
            while i < len && chars[i] != q {
                if chars[i] == '\\' { i += 1; }
                i += 1;
            }
            i += 1;
            continue;
        }
        // 跳过单行注释
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' { i += 1; }
            continue;
        }
        // L4: 跳过块注释 /* ... */
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
            i = (i + 2).min(len);
            continue;
        }
        // 识别 `ident : ExtType`
        if chars[i] == ':' && i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            let mut s = i;
            while s > 0 && (chars[s - 1].is_alphanumeric() || chars[s - 1] == '_') { s -= 1; }
            let name: String = chars[s..i].iter().collect();
            let mut j = i + 1;
            while j < len && (chars[j] == ' ' || chars[j] == '\t') { j += 1; }
            let mut t = j;
            while t < len && (chars[t].is_alphanumeric() || chars[t] == '_') { t += 1; }
            let ty: String = chars[j..t].iter().collect();
            // L4: 三元 `a?b:u8` 中 b:u8 非声明——名字紧邻前一字符为 '?' 时跳过
            let prev_is_ternary = s > 0 && chars[s - 1] == '?';
            if !name.is_empty() && !prev_is_ternary && is_ext_numeric_type(&ty) {
                map.insert(name, ty);
            }
            i = t;
            continue;
        }
        i += 1;
    }

    // H2: f32 变量被赋予小数字面量 → 标记哨兵，codegen 显式报错（避免静默 ×100 错值）
    let f32_names: Vec<String> = map.iter()
        .filter(|(_, t)| t.as_str() == "f32")
        .map(|(n, _)| n.clone())
        .collect();
    for name in f32_names {
        if f32_assigned_decimal(&chars, &name) {
            map.insert(name, F32_DECIMAL_SENTINEL.to_string());
        }
    }

    // H1: 保守回退——同名存在普通声明则剔除，消除跨作用域静默误截断
    let plain = collect_plain_decl_names(&chars);
    map.retain(|name, _| !plain.contains(name));

    map
}

/// H2: 判断名为 `name` 的变量在源码中是否被赋予小数字面量（`name [: T] = <digits>.<digits>`）。
fn f32_assigned_decimal(chars: &[char], name: &str) -> bool {
    let n = chars.len();
    let nlen = name.chars().count();
    let mut i = 0;
    while i < n {
        if starts_with_word(chars, i, name) {
            let mut j = i + nlen;
            while j < n && chars[j].is_whitespace() { j += 1; }
            // 可选类型注解 `: f32`
            if j < n && chars[j] == ':' {
                j += 1;
                while j < n && chars[j].is_whitespace() { j += 1; }
                while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                while j < n && chars[j].is_whitespace() { j += 1; }
            }
            // 期望赋值 '='（排除 '=='）
            if j < n && chars[j] == '=' && !(j + 1 < n && chars[j + 1] == '=') {
                j += 1;
                while j < n && chars[j].is_whitespace() { j += 1; }
                let ds = j;
                while j < n && chars[j].is_ascii_digit() { j += 1; }
                if j > ds && j < n && chars[j] == '.' && j + 1 < n && chars[j + 1].is_ascii_digit() {
                    return true;
                }
            }
            i += nlen;
            continue;
        }
        i += 1;
    }
    false
}

/// H1: 收集“普通（无扩展注解）声明”的变量名：let/const/var 声明与 function 形参。
/// 仅当声明处未紧跟 `: ExtType` 时计入；带扩展注解的声明不计（避免误剔除合法宽度类型）。
/// 注：箭头回调形参恒为 int32_t（不参与宽度发射），故不纳入，规避 `(expr)` 与 `(params)=>` 歧义。
fn collect_plain_decl_names(chars: &[char]) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        // 跳过字符串
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let q = chars[i];
            i += 1;
            while i < n && chars[i] != q { if chars[i] == '\\' { i += 1; } i += 1; }
            i += 1;
            continue;
        }
        // 跳过注释
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' { i += 1; }
            continue;
        }
        if i + 1 < n && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
            i = (i + 2).min(n);
            continue;
        }
        // function NAME(params)
        if starts_with_word(chars, i, "function") {
            let mut j = i + 8;
            while j < n && chars[j].is_whitespace() { j += 1; }
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; } // 函数名
            while j < n && chars[j].is_whitespace() { j += 1; }
            if j < n && chars[j] == '(' {
                j += 1;
                while j < n && chars[j] != ')' {
                    while j < n && (chars[j].is_whitespace() || chars[j] == ',') { j += 1; }
                    if j >= n || chars[j] == ')' { break; }
                    let ps = j;
                    while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                    if j > ps {
                        let pname: String = chars[ps..j].iter().collect();
                        // 先判定是否 `: ExtType`，再跳过任意 `: type` 注解推进 j
                        let ext_ann = annotated_at(chars, j);
                        let mut k = j;
                        while k < n && chars[k].is_whitespace() { k += 1; }
                        if k < n && chars[k] == ':' {
                            k += 1;
                            while k < n && chars[k].is_whitespace() { k += 1; }
                            while k < n && (chars[k].is_alphanumeric() || chars[k] == '_') { k += 1; }
                        }
                        j = k;
                        if !ext_ann { set.insert(pname); }
                    } else { j += 1; }
                }
            }
            i = j.max(i + 1);
            continue;
        }
        // let / const / var NAME
        let kwlen = if starts_with_word(chars, i, "const") { 5 }
            else if starts_with_word(chars, i, "let") { 3 }
            else if starts_with_word(chars, i, "var") { 3 }
            else { 0 };
        if kwlen > 0 {
            let mut j = i + kwlen;
            while j < n && chars[j].is_whitespace() { j += 1; }
            // 解构 let { .. } / let [ .. ] 不计
            if j < n && (chars[j] == '{' || chars[j] == '[') { i = j; continue; }
            let s = j;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
            if j > s {
                let name: String = chars[s..j].iter().collect();
                if !annotated_at(chars, j) { set.insert(name); }
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    set
}

/// 若 pos 处为 `: ExtType` 注解，返回注解之后的位置；否则原样返回 pos。
fn skip_if_ext_annotated(chars: &[char], pos: usize) -> usize {
    let n = chars.len();
    let mut k = pos;
    while k < n && chars[k].is_whitespace() { k += 1; }
    if k < n && chars[k] == ':' {
        k += 1;
        while k < n && chars[k].is_whitespace() { k += 1; }
        let ts = k;
        while k < n && (chars[k].is_alphanumeric() || chars[k] == '_') { k += 1; }
        let ty: String = chars[ts..k].iter().collect();
        if is_ext_numeric_type(&ty) { return k; }
    }
    pos
}

/// 判断 pos（名字之后）是否紧跟 `: ExtType` 扩展注解。
fn annotated_at(chars: &[char], pos: usize) -> bool {
    skip_if_ext_annotated(chars, pos) != pos
}

/// 扩展 2: 寄存器位域定义（旁路，不进 AST）
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterDef {
    /// 寄存器基地址
    pub base: u32,
    /// 位域列表 (name, hi, lo)，hi >= lo
    pub fields: Vec<(String, u8, u8)>,
}

impl RegisterDef {
    /// 查位域 (hi, lo)
    pub fn field(&self, name: &str) -> Option<(u8, u8)> {
        self.fields.iter().find(|(n, _, _)| n == name).map(|(_, hi, lo)| (*hi, *lo))
    }
    /// 位域掩码 ((1<<width)-1)
    pub fn mask(&self, name: &str) -> Option<u32> {
        self.field(name).map(|(hi, lo)| {
            let width = (hi - lo) as u32 + 1;
            if width >= 32 { 0xFFFF_FFFF } else { (1u32 << width) - 1 }
        })
    }
}

/// 解析无符号整数字面量（支持 0x/0X 十六进制与下划线分隔）
fn parse_uint_literal(s: &str) -> Option<u32> {
    let s: String = s.chars().filter(|c| *c != '_').collect();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(h, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

/// 解析位域范围 `[hi, lo]` → (hi, lo, 新位置)
fn parse_bit_range(chars: &[char], mut j: usize) -> Option<(u8, u8, usize)> {
    let n = chars.len();
    while j < n && chars[j].is_whitespace() { j += 1; }
    if j >= n || chars[j] != '[' { return None; }
    j += 1;
    while j < n && chars[j].is_whitespace() { j += 1; }
    let s = j;
    while j < n && chars[j].is_ascii_digit() { j += 1; }
    let hi: u8 = chars[s..j].iter().collect::<String>().parse().ok()?;
    while j < n && (chars[j].is_whitespace() || chars[j] == ',') { j += 1; }
    let s = j;
    while j < n && chars[j].is_ascii_digit() { j += 1; }
    let lo: u8 = chars[s..j].iter().collect::<String>().parse().ok()?;
    while j < n && chars[j] != ']' { j += 1; }
    if j < n { j += 1; }
    // L2: 位域索引须落在 32 位寄存器内；hi<lo 或 hi>31 均非法（防 >> 越位 UB）
    if hi < lo || hi > 31 { return None; }
    Some((hi, lo, j))
}

/// 旁路提取寄存器声明（扩展 2）。
/// 扫描 `@register(BASE) const|let NAME = { field: [hi, lo], ... }`，返回 NAME→RegisterDef。
/// 不修改源码、不进 AST：寄存器信息经此表旁路送入 codegen，源码零指针。
pub fn extract_registers(source: &str) -> std::collections::HashMap<String, RegisterDef> {
    let mut map = std::collections::HashMap::new();
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        // 跳过字符串
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let q = chars[i];
            i += 1;
            while i < n && chars[i] != q {
                if chars[i] == '\\' { i += 1; }
                i += 1;
            }
            i += 1;
            continue;
        }
        // 匹配 @register
        if chars[i] == '@' && starts_with_word(&chars, i + 1, "register") {
            let mut j = i + 1 + 8; // "register".len()
            while j < n && chars[j].is_whitespace() { j += 1; }
            let mut base: u32 = 0;
            if j < n && chars[j] == '(' {
                j += 1;
                while j < n && chars[j].is_whitespace() { j += 1; }
                let s = j;
                while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                base = parse_uint_literal(&chars[s..j].iter().collect::<String>()).unwrap_or(0);
                while j < n && chars[j] != ')' { j += 1; }
                if j < n { j += 1; }
            }
            while j < n && chars[j].is_whitespace() { j += 1; }
            let ks = j;
            while j < n && chars[j].is_alphabetic() { j += 1; }
            let kw: String = chars[ks..j].iter().collect();
            if kw != "const" && kw != "let" { i = j.max(i + 1); continue; }
            while j < n && chars[j].is_whitespace() { j += 1; }
            let ns = j;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
            let name: String = chars[ns..j].iter().collect();
            while j < n && chars[j].is_whitespace() { j += 1; }
            if j < n && chars[j] == '=' { j += 1; }
            while j < n && chars[j].is_whitespace() { j += 1; }
            let mut fields: Vec<(String, u8, u8)> = Vec::new();
            if j < n && chars[j] == '{' {
                j += 1;
                loop {
                    while j < n && (chars[j].is_whitespace() || chars[j] == ',') { j += 1; }
                    if j >= n { break; }
                    if chars[j] == '}' { j += 1; break; }
                    // 字段名：标识符或字符串
                    let fname: String;
                    if chars[j] == '"' || chars[j] == '\'' {
                        let q = chars[j];
                        let fs = j + 1;
                        j += 1;
                        while j < n && chars[j] != q { j += 1; }
                        fname = chars[fs..j].iter().collect();
                        if j < n { j += 1; }
                    } else {
                        let fs = j;
                        while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                        fname = chars[fs..j].iter().collect();
                    }
                    while j < n && chars[j].is_whitespace() { j += 1; }
                    if j < n && chars[j] == ':' { j += 1; }
                    match parse_bit_range(&chars, j) {
                        Some((hi, lo, nj)) => { fields.push((fname, hi, lo)); j = nj; }
                        None => break,
                    }
                }
            }
            if !name.is_empty() {
                map.insert(name, RegisterDef { base, fields });
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    map
}

fn starts_with_word(chars: &[char], pos: usize, word: &str) -> bool {
    let wchars: Vec<char> = word.chars().collect();
    if pos + wchars.len() > chars.len() { return false; }
    // 确保是行首或前面是空白
    if pos > 0 && chars[pos - 1].is_alphanumeric() { return false; }
    for (j, wc) in wchars.iter().enumerate() {
        if chars[pos + j] != *wc { return false; }
    }
    // 确保后面不是标识符字符
    let after = pos + wchars.len();
    if after < chars.len() && (chars[after].is_alphanumeric() || chars[after] == '_') {
        return false;
    }
    true
}

fn is_type_alias(chars: &[char], pos: usize) -> bool {
    // "type Foo = ..." 检查：type 后面是标识符 + 空格 + =
    let mut i = pos + 4;
    skip_whitespace(chars, &mut i);
    if i >= chars.len() || !(chars[i].is_alphabetic() || chars[i] == '_') {
        return false;
    }
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
    skip_whitespace(chars, &mut i);
    i < chars.len() && chars[i] == '='
}

fn skip_whitespace(chars: &[char], i: &mut usize) {
    while *i < chars.len() && (chars[*i] == ' ' || chars[*i] == '\t') {
        *i += 1;
    }
}

fn skip_until_block_end(chars: &[char], i: &mut usize) {
    // 跳到 '{' 然后匹配到 '}'
    while *i < chars.len() && chars[*i] != '{' { *i += 1; }
    if *i >= chars.len() { return; }
    let mut depth = 0;
    loop {
        if *i >= chars.len() { break; }
        if chars[*i] == '{' { depth += 1; }
        if chars[*i] == '}' { depth -= 1; if depth == 0 { *i += 1; break; } }
        *i += 1;
    }
}

fn skip_type_expr(chars: &[char], i: &mut usize) {
    // 跳过类型表达式: number, string, Foo<Bar>, () => void, 等
    let mut depth = 0; // for < > nesting
    let mut paren_depth = 0;
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '<' => { depth += 1; *i += 1; }
            '>' if depth > 0 => { depth -= 1; *i += 1; }
            '(' => { paren_depth += 1; *i += 1; }
            ')' if paren_depth > 0 => { paren_depth -= 1; *i += 1; }
            '=' if depth == 0 && paren_depth == 0 && peek_char(chars, *i + 1) == Some('>') => {
                // () => void 箭头
                *i += 2;
            }
            '|' | '&' if depth == 0 && paren_depth == 0 => {
                *i += 1;
                skip_whitespace(chars, i);
            }
            '[' => { *i += 1; }
            ']' => { *i += 1; }
            _ if c.is_alphanumeric() || c == '_' || c == '.' => { *i += 1; }
            ' ' | '\t' => { *i += 1; }
            _ => break,
        }
    }
}

fn peek_char(chars: &[char], pos: usize) -> Option<char> {
    if pos < chars.len() { Some(chars[pos]) } else { None }
}

fn is_type_annotation_context(chars: &[char], colon_pos: usize) -> bool {
    // 判断 ':' 是类型注解还是对象键值对
    // 类型注解场景：标识符后、参数列表中、函数返回值
    if colon_pos == 0 { return false; }
    let prev = chars[colon_pos - 1];
    // 标识符/括号后的冒号 → 类型注解
    prev.is_alphanumeric() || prev == '_' || prev == ')' || prev == '?'
}

fn is_generic_context(chars: &[char], lt_pos: usize) -> bool {
    // '<' 前面是标识符 → 可能是泛型
    if lt_pos == 0 { return false; }
    let prev = chars[lt_pos - 1];
    prev.is_alphanumeric() || prev == '_'
}

fn find_generic_end(chars: &[char], start: usize) -> Option<usize> {
    // 从 '<' 开始找匹配的 '>'
    let mut depth = 0;
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '<' { depth += 1; }
        if chars[i] == '>' { depth -= 1; if depth == 0 { return Some(i); } }
        if chars[i] == ';' || chars[i] == '{' { return None; } // 不是泛型
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_variable_annotation() {
        let input = "let x: number = 42;";
        let output = strip_types(input);
        assert!(output.contains("let x = 42;"), "got: {}", output);
    }

    #[test]
    fn test_strip_function_params() {
        let input = "function add(a: number, b: number): number { return a + b; }";
        let output = strip_types(input);
        assert!(output.contains("function add(a, b)"), "got: {}", output);
        assert!(output.contains("{ return a + b; }"), "got: {}", output);
    }

    #[test]
    fn test_strip_interface() {
        let input = "interface Foo { x: number; }\nlet a = 10;";
        let output = strip_types(input);
        assert!(!output.contains("interface"));
        assert!(output.contains("let a = 10;"));
    }

    #[test]
    fn test_strip_type_alias() {
        let input = "type ID = number;\nlet id = 1;";
        let output = strip_types(input);
        assert!(!output.contains("type ID"));
        assert!(output.contains("let id = 1;"));
    }

    #[test]
    fn test_preserve_strings() {
        let input = r#"let s: string = "hello: world";"#;
        let output = strip_types(input);
        assert!(output.contains(r#""hello: world""#), "got: {}", output);
    }

    // 扩展 1: 宽度注解旁路提取
    #[test]
    fn test_extract_width_let() {
        let w = extract_width_types("let r: u8 = 255; let c: u16 = 300;");
        assert_eq!(w.get("r").map(|s| s.as_str()), Some("u8"));
        assert_eq!(w.get("c").map(|s| s.as_str()), Some("u16"));
    }

    #[test]
    fn test_extract_width_params() {
        let w = extract_width_types("function f(a: u8, b: u32) { return a; }");
        assert_eq!(w.get("a").map(|s| s.as_str()), Some("u8"));
        assert_eq!(w.get("b").map(|s| s.as_str()), Some("u32"));
    }

    #[test]
    fn test_extract_ignores_plain_types() {
        let w = extract_width_types("let x: number = 1; let s: string = \"u8: hi\";");
        assert!(w.is_empty(), "got: {:?}", w);
    }

    // 扩展 2: 寄存器位域旁路提取
    #[test]
    fn test_extract_register_basic() {
        let src = "@register(0x40004000) const GPIO_OUT = { en: [1, 1], val: [7, 2] };";
        let regs = extract_registers(src);
        let def = regs.get("GPIO_OUT").expect("GPIO_OUT registered");
        assert_eq!(def.base, 0x40004000);
        assert_eq!(def.field("en"), Some((1, 1)));
        assert_eq!(def.field("val"), Some((7, 2)));
        assert_eq!(def.mask("val"), Some(0x3F));
        assert_eq!(def.mask("en"), Some(0x1));
    }

    #[test]
    fn test_extract_register_ignores_plain_const() {
        let regs = extract_registers("const X = { a: 1 }; let y = 2;");
        assert!(regs.is_empty(), "got: {:?}", regs);
    }
}
