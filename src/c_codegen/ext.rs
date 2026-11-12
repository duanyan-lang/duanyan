//! 扩展 1-4 代码生成 helpers（任务 B2 从 c_codegen/mod.rs 拆出，纯搬运）
use super::*;

impl CCodeGenerator {
    /// 扩展 1: 变量/参数的 C 类型（按旁路宽度表；缺省 int32_t）
    pub(super) fn c_width_type(&self, name: &str) -> &'static str {
        match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => "uint8_t",
            Some("u16") => "uint16_t",
            Some("u32") => "uint32_t",
            Some("i8") => "int8_t",
            Some("i16") => "int16_t",
            Some("q15") | Some("q16") => "int32_t", // 扩展 3: 定点以 int32_t 原始存储
            Some("f32") => "float",                  // 扩展 3: 受控浮点逃生舱（有 FPU 目标）
            Some(F32_DECIMAL_SENTINEL) => "float",   // H2: f32 但曾赋小数字面量（存储仍 float，字面量处报错）
            _ => "int32_t",
        }
    }

    /// 扩展 1: 宽度变量的截断转换前缀（缺省 int32_t 无需截断 → None）
    pub(super) fn width_cast(&self, name: &str) -> Option<&'static str> {
        match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => Some("(uint8_t)"),
            Some("u16") => Some("(uint16_t)"),
            Some("u32") => Some("(uint32_t)"),
            Some("i8") => Some("(int8_t)"),
            Some("i16") => Some("(int16_t)"),
            Some("q15") | Some("q16") => Some("(int32_t)"), // 扩展 3: 定点截断
            Some("f32") => Some("(float)"),                  // 扩展 3: 浮点转换
            Some(F32_DECIMAL_SENTINEL) => Some("(float)"),   // H2: 同上（非字面量赋值仍转 float）
            _ => None,
        }
    }

    /// 扩展 3: 若表达式为 q15/q16 定点标识符，返回其移位量 (15/16)
    pub(super) fn fixed_shift_of(&self, e: &Expr) -> Option<u32> {
        if let Expr::Ident(name) = e {
            match self.width_types.get(name).map(|s| s.as_str()) {
                Some("q15") => return Some(15),
                Some("q16") => return Some(16),
                _ => {}
            }
        }
        None
    }

    /// H2: f32 变量被赋予数字字面量时的编译期校验。
    /// 词法层将小数 ×100 缩放为整数，Number token 无法区分 1.5 与 150；
    /// 若该 f32 名在源码中曾被赋小数字面量（标记 F32_DECIMAL_SENTINEL），则显式报错，避免静默 ×100 错值。
    pub(super) fn check_f32_decimal(&self, name: &str, value: &Expr) -> Result<(), String> {
        if matches!(value, Expr::Number(_))
            && self.width_types.get(name).map(|s| s.as_str()) == Some(F32_DECIMAL_SENTINEL)
        {
            return Err(format!(
                "f32 变量 '{}' 被赋予小数字面量：DUANYAN 词法层将小数统一 ×100 缩放为整数，无法忠实表示真浮点值。请改用 q15/q16 定点类型，或避免对 f32 直接使用小数字面量。", name));
        }
        Ok(())
    }

    /// 扩展 4: 数组长度静态查询（仅静态已知长度可降级为确定性循环）
    pub(super) fn array_len(&self, name: &str) -> Option<usize> {
        self.array_vars.iter().find(|(n, _)| n == name).map(|(_, l)| *l)
    }

    /// 扩展 4: 匹配数组高阶方法调用 → (数组名, 方法, 参数)
    pub(super) fn match_array_hof(value: &Expr) -> Option<(String, String, &[Expr])> {
        match value {
            Expr::Call { name, args } => {
                let (a, m) = name.split_once('.')?;
                if matches!(m, "map" | "filter" | "forEach" | "reduce") {
                    Some((a.to_string(), m.to_string(), args))
                } else {
                    None
                }
            }
            Expr::StringMethod { object, method, args } if method == "slice" => {
                if let Expr::Ident(a) = object.as_ref() {
                    Some((a.clone(), "slice".to_string(), args))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 扩展 4: 数组高阶方法降级为确定性 C for 循环（静态缓冲，回调内联，回调次数=数组长度）
    pub(super) fn emit_array_hof(&mut self, name: Option<String>, arr: &str, len: usize, method: &str, args: &[Expr]) -> Result<String, String> {
        let pad = self.indent_str();
        self.arrow_counter += 1;
        let idx = format!("_hi{}", self.arrow_counter);
        self.needs_stdint = true;

        // H3: filter/slice 结果为运行期长度，禁止再作高阶方法源（编译期长度不确定，无法静态降级为确定性循环）
        if self.dynamic_len_arrays.contains(arr) {
            return Err(format!("扩展 4: 暂不支持对 filter/slice 结果 '{}' 再做数组高阶方法（其长度为运行期值，无法静态降级）", arr));
        }

        // slice 无回调：拷贝子区间（M1: 起止索引按 JS 语义 clamp，防负索引越界读）
        if method == "slice" {
            let name = name.unwrap_or_else(|| format!("_hs{}", self.arrow_counter));
            let a_src = if !args.is_empty() { self.generate_expr(&args[0])? } else { "0".to_string() };
            let b_src = if args.len() > 1 { self.generate_expr(&args[1])? } else { len.to_string() };
            let sa = format!("_hsa{}", self.arrow_counter);
            let sb = format!("_hsb{}", self.arrow_counter);
            self.array_vars.push((name.clone(), len));
            self.dynamic_len_arrays.insert(name.clone()); // H3: slice 结果为运行期长度
            let mut code = String::new();
            code.push_str(&format!("{}static int32_t {}[{}];\n", pad, name, len));
            code.push_str(&format!("{}int32_t {}_len = 0;\n", pad, name));
            code.push_str(&format!("{}int32_t {sa} = {a}; if ({sa} < 0) {{ {sa} += {len}; }} if ({sa} < 0) {{ {sa} = 0; }} if ({sa} > {len}) {{ {sa} = {len}; }}\n", pad, sa = sa, a = a_src, len = len));
            code.push_str(&format!("{}int32_t {sb} = {b}; if ({sb} < 0) {{ {sb} += {len}; }} if ({sb} < 0) {{ {sb} = 0; }} if ({sb} > {len}) {{ {sb} = {len}; }}\n", pad, sb = sb, b = b_src, len = len));
            code.push_str(&format!("{}for (int32_t {i} = {sa}; {i} < {sb}; {i}++) {{\n", pad, i = idx, sa = sa, sb = sb));
            code.push_str(&format!("{}    {}[{}_len++] = {}[{}];\n", pad, name, name, arr, idx));
            code.push_str(&format!("{}}}\n", pad));
            return Ok(code);
        }

        // 其余需回调箭头函数
        let (params, body) = match args.first() {
            Some(Expr::ArrowFunc { params, body, .. }) => (params.clone(), body),
            _ => return Err(format!("扩展 4: {}() 回调必须是箭头函数", method)),
        };

        match method {
            "forEach" => {
                let p = params.first().cloned().unwrap_or_else(|| "_x".to_string());
                let mut code = String::new();
                code.push_str(&format!("{}for (int32_t {i} = 0; {i} < {len}; {i}++) {{\n", pad, i = idx, len = len));
                code.push_str(&format!("{}    int32_t {} = {}[{}];\n", pad, p, arr, idx));
                match body {
                    ArrowBody::Expr(e) => {
                        let be = self.generate_expr(e)?;
                        code.push_str(&format!("{}    {};\n", pad, be));
                    }
                    ArrowBody::Block(stmts) => {
                        let saved = self.indent;
                        self.indent = saved + 1;
                        for s in stmts {
                            let line = self.generate_statement(&s.stmt)?;
                            code.push_str(&line);
                        }
                        self.indent = saved;
                    }
                }
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            "map" => {
                let name = name.unwrap_or_else(|| format!("_hm{}", self.arrow_counter));
                let p = params.first().cloned().unwrap_or_else(|| "_x".to_string());
                let be = match body {
                    ArrowBody::Expr(e) => self.generate_expr(e)?,
                    ArrowBody::Block(_) => return Err("扩展 4: map 回调需表达式体 (x => expr)".to_string()),
                };
                self.array_vars.push((name.clone(), len));
                let mut code = String::new();
                code.push_str(&format!("{}static int32_t {}[{}];\n", pad, name, len));
                code.push_str(&format!("{}for (int32_t {i} = 0; {i} < {len}; {i}++) {{\n", pad, i = idx, len = len));
                code.push_str(&format!("{}    int32_t {} = {}[{}];\n", pad, p, arr, idx));
                code.push_str(&format!("{}    {}[{}] = {};\n", pad, name, idx, be));
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            "filter" => {
                let name = name.unwrap_or_else(|| format!("_hf{}", self.arrow_counter));
                let p = params.first().cloned().unwrap_or_else(|| "_x".to_string());
                let pred = match body {
                    ArrowBody::Expr(e) => self.generate_expr(e)?,
                    ArrowBody::Block(_) => return Err("扩展 4: filter 回调需表达式体".to_string()),
                };
                self.array_vars.push((name.clone(), len));
                self.dynamic_len_arrays.insert(name.clone()); // H3: filter 结果为运行期长度
                let mut code = String::new();
                code.push_str(&format!("{}static int32_t {}[{}];\n", pad, name, len));
                code.push_str(&format!("{}int32_t {}_len = 0;\n", pad, name));
                code.push_str(&format!("{}for (int32_t {i} = 0; {i} < {len}; {i}++) {{\n", pad, i = idx, len = len));
                code.push_str(&format!("{}    int32_t {} = {}[{}];\n", pad, p, arr, idx));
                code.push_str(&format!("{}    if ({}) {{ {}[{}_len++] = {}[{}]; }}\n", pad, pred, name, name, arr, idx));
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            "reduce" => {
                let name = name.unwrap_or_else(|| format!("_hr{}", self.arrow_counter));
                let acc = params.first().cloned().unwrap_or_else(|| "_acc".to_string());
                let elem = params.get(1).cloned().unwrap_or_else(|| "_x".to_string());
                if acc == elem {
                    return Err("扩展 4: reduce 回调的两个参数（累加器/元素）不能同名".to_string());
                }
                let init = if args.len() > 1 { self.generate_expr(&args[1])? } else { "0".to_string() };
                let be = match body {
                    ArrowBody::Expr(e) => self.generate_expr(e)?,
                    ArrowBody::Block(_) => return Err("扩展 4: reduce 回调需表达式体".to_string()),
                };
                let mut code = String::new();
                code.push_str(&format!("{}int32_t {} = {};\n", pad, acc, init));
                code.push_str(&format!("{}for (int32_t {i} = 0; {i} < {len}; {i}++) {{\n", pad, i = idx, len = len));
                code.push_str(&format!("{}    int32_t {} = {}[{}];\n", pad, elem, arr, idx));
                code.push_str(&format!("{}    {} = {};\n", pad, acc, be));
                code.push_str(&format!("{}}}\n", pad));
                // M3: 累加器名与结果名相同时，累加器即结果，不再重复声明（防 C 变量重定义）
                if name != acc {
                    code.push_str(&format!("{}int32_t {} = {};\n", pad, name, acc));
                }
                Ok(code)
            }
            _ => Err(format!("扩展 4: 不支持的数组方法 {}", method)),
        }
    }

    /// 扩展 1: 字面量越界编译期 lint（超宽度范围 → 编译错误）
    pub(super) fn lint_width_literal(&self, name: &str, n: i64) -> Result<(), String> {
        let (lo, hi, tn) = match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => (0i128, 255i128, "u8"),
            Some("u16") => (0, 65535, "u16"),
            Some("u32") => (0, 4294967295, "u32"),
            Some("i8") => (-128, 127, "i8"),
            Some("i16") => (-32768, 32767, "i16"),
            Some("i32") => (-2147483648, 2147483647, "i32"),
            _ => return Ok(()),
        };
        let v = n as i128;
        if v < lo || v > hi {
            return Err(format!(
                "宽度越界: 变量 '{}' 类型为 {}, 字面量 {} 超出范围 [{}, {}]",
                name, tn, n, lo, hi
            ));
        }
        Ok(())
    }

    /// 扩展 2: 寄存器位域读表达式 → ((DUANYAN_REG32(base) >> lo) & mask)
    pub(super) fn register_read_expr(&mut self, reg: &str, field: &str) -> Option<String> {
        let (base, lo, mask) = {
            let def = self.registers.get(reg)?;
            let (_, lo) = def.field(field)?;
            let mask = def.mask(field)?;
            (def.base, lo, mask)
        };
        self.needs_register = true;
        Some(format!("((DUANYAN_REG32(0x{:08X}) >> {}) & 0x{:X})", base, lo, mask))
    }

    /// 扩展 2: 寄存器位域写语句 → masked RMW（读-改-写，不影响其他位）
    pub(super) fn register_write_stmt(&mut self, pad: &str, reg: &str, field: &str, val: &str) -> Option<String> {
        let (base, lo, mask) = {
            let def = self.registers.get(reg)?;
            let (_, lo) = def.field(field)?;
            let mask = def.mask(field)?;
            (def.base, lo, mask)
        };
        self.needs_register = true;
        let addr = format!("0x{:08X}", base);
        // L1: 清位掩码也转 uint32_t 再移位，使整个 RMW 全程无符号良定义（避免触及 bit31 时裸 int 左移的实现定义行为）
        Some(format!(
            "{pad}DUANYAN_REG32({a}) = (DUANYAN_REG32({a}) & ~((uint32_t)0x{m:X} << {lo})) | (((uint32_t)({v}) & (uint32_t)0x{m:X}) << {lo});\n",
            a = addr, m = mask, lo = lo, v = val, pad = pad
        ))
    }

}