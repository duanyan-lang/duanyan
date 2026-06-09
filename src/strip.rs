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
}
