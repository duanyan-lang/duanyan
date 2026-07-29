//! 手写 JSON 解析器 (零依赖)
//!
//! 递归下降解析器，支持 Hardy PACK 所需的完整 JSON 子集

use std::fmt;

/// JSON 值
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// 从字符串解析 JSON
    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut parser = JsonParser::new(input);
        let val = parser.parse_value()?;
        parser.skip_ws();
        if parser.pos < parser.input.len() {
            return Err(format!("JSON 解析: 多余字符 at pos {}", parser.pos));
        }
        Ok(val)
    }

    /// 获取 Object 字段
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        if let JsonValue::Object(pairs) = self {
            pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let JsonValue::Str(s) = self { Some(s.as_str()) } else { None }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let JsonValue::Number(n) = self { Some(*n) } else { None }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let JsonValue::Bool(b) = self { Some(*b) } else { None }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        if let JsonValue::Array(a) = self { Some(a) } else { None }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, JsonValue)>> {
        if let JsonValue::Object(o) = self { Some(o) } else { None }
    }

    /// 序列化为 JSON 字符串 (pretty print)
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out, 0);
        out
    }

    fn write_json(&self, out: &mut String, indent: usize) {
        let pad = "  ".repeat(indent);
        let pad1 = "  ".repeat(indent + 1);
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Number(n) => {
                if *n == (*n as i64) as f64 && n.is_finite() {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{}", n));
                }
            }
            JsonValue::Str(s) => {
                out.push('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '\r' => out.push_str("\\r"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            JsonValue::Array(arr) => {
                if arr.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\n");
                    for (i, v) in arr.iter().enumerate() {
                        out.push_str(&pad1);
                        v.write_json(out, indent + 1);
                        if i + 1 < arr.len() { out.push(','); }
                        out.push('\n');
                    }
                    out.push_str(&pad);
                    out.push(']');
                }
            }
            JsonValue::Object(pairs) => {
                if pairs.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        out.push_str(&pad1);
                        out.push('"');
                        out.push_str(k);
                        out.push_str("\": ");
                        v.write_json(out, indent + 1);
                        if i + 1 < pairs.len() { out.push(','); }
                        out.push('\n');
                    }
                    out.push_str(&pad);
                    out.push('}');
                }
            }
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_json_string())
    }
}

// =============================================================================
// 递归下降解析器
// =============================================================================

struct JsonParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input: input.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() { self.pos += 1; }
        ch
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            match ch {
                b' ' | b'\t' | b'\r' | b'\n' => { self.pos += 1; }
                _ => break,
            }
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        self.skip_ws();
        match self.next() {
            Some(ch) if ch == expected => Ok(()),
            Some(ch) => Err(format!("JSON: 期望 '{}' 得到 '{}'", expected as char, ch as char)),
            None => Err(format!("JSON: 期望 '{}' 但到达末尾", expected as char)),
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'"') => self.parse_string().map(JsonValue::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') | Some(b'f') => self.parse_bool(),
            Some(b'n') => self.parse_null(),
            Some(ch) if ch == b'-' || ch.is_ascii_digit() => self.parse_number(),
            Some(ch) => Err(format!("JSON: 意外字符 '{}' at pos {}", ch as char, self.pos)),
            None => Err("JSON: 意外到达末尾".to_string()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.next() {
                Some(b'"') => return Ok(s),
                Some(b'\\') => {
                    match self.next() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'b') => s.push('\u{0008}'),
                        Some(b'f') => s.push('\u{000C}'),
                        Some(b'u') => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                match self.next() {
                                    Some(ch) => hex.push(ch as char),
                                    None => return Err("JSON: \\u 转义不完整".to_string()),
                                }
                            }
                            let code = u32::from_str_radix(&hex, 16)
                                .map_err(|_| format!("JSON: 无效 unicode \\u{}", hex))?;
                            if let Some(c) = char::from_u32(code) {
                                s.push(c);
                            }
                        }
                        _ => return Err("JSON: 无效转义序列".to_string()),
                    }
                }
                Some(ch) => s.push(ch as char),
                None => return Err("JSON: 未闭合的字符串".to_string()),
            }
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        // optional minus
        if self.peek() == Some(b'-') { self.pos += 1; }
        // integer part
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() { self.pos += 1; } else { break; }
        }
        // decimal part
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() { self.pos += 1; } else { break; }
            }
        }
        // exponent
        if self.peek() == Some(b'e') || self.peek() == Some(b'E') {
            self.pos += 1;
            if self.peek() == Some(b'+') || self.peek() == Some(b'-') { self.pos += 1; }
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() { self.pos += 1; } else { break; }
            }
        }
        let num_str = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| "JSON: 无效 UTF-8 数字".to_string())?;
        let n: f64 = num_str.parse()
            .map_err(|_| format!("JSON: 无效数字 '{}'", num_str))?;
        Ok(JsonValue::Number(n))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.input[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(JsonValue::Bool(true))
        } else if self.input[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(JsonValue::Bool(false))
        } else {
            Err(format!("JSON: 无效值 at pos {}", self.pos))
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        if self.input[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(JsonValue::Null)
        } else {
            Err(format!("JSON: 无效值 at pos {}", self.pos))
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect(b'[')?;
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(arr));
        }
        loop {
            arr.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b']') => { self.pos += 1; return Ok(JsonValue::Array(arr)); }
                _ => return Err("JSON: 数组中期望 ',' 或 ']'".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(pairs));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.expect(b':')?;
            let val = self.parse_value()?;
            pairs.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b'}') => { self.pos += 1; return Ok(JsonValue::Object(pairs)); }
                _ => return Err("JSON: 对象中期望 ',' 或 '}'".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primitives() {
        assert_eq!(JsonValue::parse("null").unwrap(), JsonValue::Null);
        assert_eq!(JsonValue::parse("true").unwrap(), JsonValue::Bool(true));
        assert_eq!(JsonValue::parse("false").unwrap(), JsonValue::Bool(false));
        assert_eq!(JsonValue::parse("42").unwrap(), JsonValue::Number(42.0));
        assert_eq!(JsonValue::parse("-3.14").unwrap(), JsonValue::Number(-3.14));
    }

    #[test]
    fn test_parse_string() {
        assert_eq!(JsonValue::parse(r#""hello""#).unwrap(), JsonValue::Str("hello".to_string()));
        assert_eq!(JsonValue::parse(r#""a\nb""#).unwrap(), JsonValue::Str("a\nb".to_string()));
    }

    #[test]
    fn test_parse_array() {
        let v = JsonValue::parse("[1, 2, 3]").unwrap();
        assert_eq!(v.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_parse_object() {
        let v = JsonValue::parse(r#"{"name": "test", "version": "1.0.0"}"#).unwrap();
        assert_eq!(v.get("name").unwrap().as_str().unwrap(), "test");
        assert_eq!(v.get("version").unwrap().as_str().unwrap(), "1.0.0");
    }

    #[test]
    fn test_roundtrip() {
        let json = r#"{
  "name": "my-project",
  "version": "1.0.0",
  "deps": [
    1,
    2
  ]
}"#;
        let v = JsonValue::parse(json).unwrap();
        let s = v.to_json_string();
        let v2 = JsonValue::parse(&s).unwrap();
        assert_eq!(v, v2);
    }
}
