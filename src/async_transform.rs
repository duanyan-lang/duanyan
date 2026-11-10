//! Phase v1.5: async/await 状态机变换
//!
//! 将每个 `async function` 在编译期转换为状态机结构体 + poll 函数。
//! 运行时通过事件循环驱动，零动态分配，适合资源受限 MCU。

use crate::parser::{Expr, Function, SourceStatement, Statement, ArrowBody};

/// 异步函数分析结果
#[derive(Debug, Clone)]
pub struct AsyncAnalysis {
    /// 函数名
    pub func_name: String,
    /// 每个 await 点的信息
    pub await_points: Vec<AwaitPoint>,
    /// 跨 await 存活的局部变量 (变量名, 类型大小 bytes)
    pub live_vars_across_await: Vec<LiveVar>,
    /// 状态数 = await 点数 + 2 (初始 + 完成)
    pub state_count: usize,
    /// 上下文结构体大小 (bytes)
    pub ctx_size: usize,
}

/// 单个 await 点
#[derive(Debug, Clone)]
pub struct AwaitPoint {
    /// await 的表达式
    pub expr: Expr,
    /// await 后的语句索引 (resume 位置)
    pub resume_stmt_idx: usize,
    /// 此刻存活的变量集
    pub live_vars: Vec<String>,
    /// 状态编号
    pub state_id: usize,
}

/// 跨 await 存活的变量
#[derive(Debug, Clone)]
pub struct LiveVar {
    pub name: String,
    pub size_bytes: usize,  // 默认 4 (int32)
}

/// 状态机变换后的函数表示
#[derive(Debug, Clone)]
pub struct StateMachine {
    /// 原函数名
    pub name: String,
    /// 参数列表
    pub params: Vec<String>,
    /// 状态数
    pub state_count: usize,
    /// 上下文变量 (跨 await 保存)
    pub ctx_vars: Vec<LiveVar>,
    /// 每个状态对应的代码段
    pub states: Vec<StateBlock>,
    /// 上下文结构体总大小
    pub ctx_size: usize,
}

/// 单个状态的代码块
#[derive(Debug, Clone)]
pub struct StateBlock {
    /// 状态 ID
    pub state_id: usize,
    /// 该状态中的语句
    pub statements: Vec<SourceStatement>,
    /// 该状态结束时的 await 表达式 (如果有)
    pub await_expr: Option<Expr>,
    /// 下一个状态 ID
    pub next_state: usize,
    /// 是否是最终状态
    pub is_final: bool,
}

/// 分析 async 函数，提取 await 点和跨 await 存活变量
pub fn analyze_async_function(func: &Function) -> Result<AsyncAnalysis, String> {
    if !func.is_async {
        return Err(format!("函数 {} 不是 async 函数", func.name));
    }

    let mut await_points = Vec::new();
    let mut all_vars: Vec<String> = Vec::new();
    let mut vars_before_await: Vec<Vec<String>> = Vec::new();

    // 遍历函数体，找到所有 await 点
    for (idx, ss) in func.body.iter().enumerate() {
        // 收集 let/const 声明的变量
        collect_declared_vars(&ss.stmt, &mut all_vars);

        // 检查是否包含 await
        if let Some(await_expr) = find_await_in_statement(&ss.stmt) {
            let live_vars: Vec<String> = all_vars.clone();
            await_points.push(AwaitPoint {
                expr: await_expr.clone(),
                resume_stmt_idx: idx + 1,
                live_vars: live_vars.clone(),
                state_id: await_points.len() + 1, // state 0 是初始
            });
            vars_before_await.push(live_vars);
        }
    }

    // 计算跨 await 存活的变量 (在某个 await 之前声明，在之后使用)
    let live_vars_across_await = compute_live_vars_across_await(
        &func.body, &await_points
    );

    let state_count = await_points.len() + 2; // 初始 + N个await + 完成
    // ctx 大小: 1 byte state + 4 bytes per live var + 4 bytes result
    let ctx_size = 1 + live_vars_across_await.len() * 4 + 4;

    Ok(AsyncAnalysis {
        func_name: func.name.clone(),
        await_points,
        live_vars_across_await,
        state_count,
        ctx_size,
    })
}

/// 将 async 函数变换为状态机表示
pub fn transform_to_state_machine(func: &Function) -> Result<StateMachine, String> {
    let analysis = analyze_async_function(func)?;

    let mut states = Vec::new();
    let await_count = analysis.await_points.len();

    // 按 await 点分割语句为多个状态块
    let mut stmt_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for ap in &analysis.await_points {
        stmt_ranges.push((start, ap.resume_stmt_idx));
        start = ap.resume_stmt_idx;
    }
    // 最后一个状态块: 从最后一个 await 之后到函数结束
    stmt_ranges.push((start, func.body.len()));

    for (i, (range_start, range_end)) in stmt_ranges.iter().enumerate() {
        let statements: Vec<SourceStatement> = func.body[*range_start..*range_end].to_vec();
        let await_expr = if i < await_count {
            Some(analysis.await_points[i].expr.clone())
        } else {
            None
        };
        let is_final = i == stmt_ranges.len() - 1;
        let next_state = if is_final { i } else { i + 1 };

        states.push(StateBlock {
            state_id: i,
            statements,
            await_expr,
            next_state,
            is_final,
        });
    }

    Ok(StateMachine {
        name: func.name.clone(),
        params: func.params.clone(),
        state_count: analysis.state_count,
        ctx_vars: analysis.live_vars_across_await,
        states,
        ctx_size: analysis.ctx_size,
    })
}

/// 生成 C 语言状态机代码
pub fn generate_c_state_machine(sm: &StateMachine) -> String {
    let mut code = String::new();

    // 1. 生成上下文结构体
    code.push_str(&format!("/* async function {}() — 由 DUANYAN v1.5 自动生成 */\n", sm.name));
    code.push_str(&format!("typedef struct {{\n"));
    code.push_str("    uint8_t state;\n");
    for var in &sm.ctx_vars {
        code.push_str(&format!("    int32_t {};\n", var.name));
    }
    code.push_str("    int32_t result;\n");
    code.push_str(&format!("}} {}_ctx;\n\n", sm.name));

    // 2. 生成 poll 函数
    code.push_str(&format!("duanyan_status_t {}_poll({}_ctx* ctx) {{\n", sm.name, sm.name));
    code.push_str("    switch (ctx->state) {\n");

    for state in &sm.states {
        code.push_str(&format!("        case {}: /* state {} */\n", state.state_id, state.state_id));

        // 生成语句代码 (简化: 注释占位)
        for ss in &state.statements {
            code.push_str(&format!("            /* {} */\n", format_statement_brief(&ss.stmt)));
        }

        if let Some(ref await_expr) = state.await_expr {
            // 发起异步操作
            code.push_str(&format!("            /* await: {} */\n", format_expr_brief(await_expr)));
            code.push_str(&format!("            ctx->state = {};\n", state.next_state));
            code.push_str("            return DUANYAN_PENDING;\n");
        } else if state.is_final {
            code.push_str("            ctx->state = 255; /* done */\n");
            code.push_str("            return DUANYAN_READY;\n");
        }

        code.push_str("\n");
    }

    code.push_str("    }\n");
    code.push_str("    return DUANYAN_READY;\n");
    code.push_str("}\n");

    code
}

/// 生成 C 事件循环代码
pub fn generate_c_event_loop(target: &str) -> String {
    let mut code = String::new();

    code.push_str("/* duanyan_event_loop.h — 由 DUANYAN v1.5 自动生成 */\n");
    code.push_str("#ifndef DUANYAN_EVENT_LOOP_H\n");
    code.push_str("#define DUANYAN_EVENT_LOOP_H\n\n");
    code.push_str("#include <stdint.h>\n");
    code.push_str("#include <stdbool.h>\n\n");

    // 状态枚举
    code.push_str("typedef enum {\n");
    code.push_str("    DUANYAN_PENDING = 0,\n");
    code.push_str("    DUANYAN_READY = 1,\n");
    code.push_str("    DUANYAN_ERROR = -1\n");
    code.push_str("} duanyan_status_t;\n\n");

    // 任务结构体
    code.push_str("typedef struct duanyan_task {\n");
    code.push_str("    duanyan_status_t (*poll)(void* ctx);\n");
    code.push_str("    void* ctx;\n");
    code.push_str("    struct duanyan_task* next;\n");
    code.push_str("} duanyan_task_t;\n\n");

    // 任务链表
    code.push_str("static duanyan_task_t* duanyan_task_head = NULL;\n\n");

    // 注册任务
    code.push_str("void duanyan_spawn(duanyan_status_t (*poll_fn)(void*), void* ctx) {\n");
    code.push_str("    static duanyan_task_t tasks[16];\n");
    code.push_str("    static int task_count = 0;\n");
    code.push_str("    if (task_count >= 16) return;\n");
    code.push_str("    duanyan_task_t* t = &tasks[task_count++];\n");
    code.push_str("    t->poll = poll_fn;\n");
    code.push_str("    t->ctx = ctx;\n");
    code.push_str("    t->next = duanyan_task_head;\n");
    code.push_str("    duanyan_task_head = t;\n");
    code.push_str("}\n\n");

    // 事件循环
    code.push_str("void duanyan_event_loop_run(void) {\n");
    code.push_str("    while (duanyan_task_head) {\n");
    code.push_str("        duanyan_task_t** pp = &duanyan_task_head;\n");
    code.push_str("        while (*pp) {\n");
    code.push_str("            duanyan_task_t* t = *pp;\n");
    code.push_str("            duanyan_status_t s = t->poll(t->ctx);\n");
    code.push_str("            if (s == DUANYAN_READY || s == DUANYAN_ERROR) {\n");
    code.push_str("                *pp = t->next; /* 移除完成的任务 */\n");
    code.push_str("            } else {\n");
    code.push_str("                pp = &t->next;\n");
    code.push_str("            }\n");
    code.push_str("        }\n");

    match target {
        "esp32" => {
            code.push_str("        vTaskDelay(1); /* FreeRTOS: 让出 CPU 1 tick */\n");
        }
        "stm32" => {
            code.push_str("        HAL_Delay(1); /* STM32 HAL: 延时 1ms */\n");
        }
        _ => {
            // Arduino 裸轮询，无延时
            code.push_str("        /* Arduino: 裸轮询，无延时 */\n");
        }
    }

    code.push_str("    }\n");
    code.push_str("}\n\n");
    code.push_str("#endif /* DUANYAN_EVENT_LOOP_H */\n");

    code
}

// === 内部辅助函数 ===

/// 收集语句中声明的变量名
fn collect_declared_vars(stmt: &Statement, vars: &mut Vec<String>) {
    match stmt {
        Statement::Let { name, .. } => vars.push(name.clone()),
        Statement::Const { name, .. } => vars.push(name.clone()),
        Statement::DestructLet { names, .. } => vars.extend(names.iter().cloned()),
        _ => {}
    }
}

/// 在语句中查找 await 表达式
fn find_await_in_statement(stmt: &Statement) -> Option<&Expr> {
    match stmt {
        Statement::Let { value, .. } | Statement::Const { value, .. } | Statement::Assign { value, .. } => {
            find_await_in_expr(value)
        }
        Statement::Return(expr) => find_await_in_expr(expr),
        Statement::Expr(expr) => find_await_in_expr(expr),
        _ => None,
    }
}

/// 在表达式中递归查找 await
fn find_await_in_expr(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Await { expr: inner } => Some(inner),
        Expr::BinOp { left, right, .. } => {
            find_await_in_expr(left).or_else(|| find_await_in_expr(right))
        }
        Expr::Call { args, .. } => {
            args.iter().find_map(|a| find_await_in_expr(a))
        }
        Expr::PromiseAll { promises } | Expr::PromiseRace { promises } => {
            // Promise.all/race 本身就是一个 await 点
            Some(expr)
        }
        _ => None,
    }
}

/// 计算跨 await 存活的变量
fn compute_live_vars_across_await(
    body: &[SourceStatement],
    await_points: &[AwaitPoint],
) -> Vec<LiveVar> {
    let mut live_vars: Vec<LiveVar> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for ap in await_points {
        for var_name in &ap.live_vars {
            if !seen.contains(var_name) {
                seen.push(var_name.clone());
                live_vars.push(LiveVar {
                    name: var_name.clone(),
                    size_bytes: 4, // int32
                });
            }
        }
    }

    live_vars
}

/// 格式化语句简述 (用于 C 注释)
fn format_statement_brief(stmt: &Statement) -> String {
    match stmt {
        Statement::Let { name, .. } => format!("let {} = ...", name),
        Statement::Const { name, .. } => format!("const {} = ...", name),
        Statement::Assign { name, .. } => format!("{} = ...", name),
        Statement::Return(_) => "return ...".to_string(),
        Statement::If { .. } => "if (...)".to_string(),
        Statement::While { .. } => "while (...)".to_string(),
        Statement::ConsoleLog(_) => "console.log(...)".to_string(),
        Statement::Expr(_) => "expr;".to_string(),
        _ => "...".to_string(),
    }
}

/// 格式化表达式简述 (用于 C 注释)
fn format_expr_brief(expr: &Expr) -> String {
    match expr {
        Expr::Call { name, .. } => format!("{}(...)", name),
        Expr::Ident(name) => name.clone(),
        Expr::PromiseAll { promises } => format!("Promise.all([{} items])", promises.len()),
        Expr::PromiseRace { promises } => format!("Promise.race([{} items])", promises.len()),
        _ => "expr".to_string(),
    }
}

// =========================================================================
// Phase 3.4: 无栈协程分析
// =========================================================================

/// 协程帧信息 (Phase 3.4)
#[derive(Debug, Clone)]
pub struct CoroutineFrame {
    /// 函数名
    pub func_name: String,
    /// 帧大小 (bytes) — 编译期确定
    pub frame_size: usize,
    /// 状态数
    pub state_count: usize,
    /// 是否零动态分配
    pub zero_alloc: bool,
    /// 是否支持 @realtime (有界 poll)
    pub realtime_safe: bool,
}

/// 分析协程帧大小 (Phase 3.4)
pub fn analyze_coroutine_frame(func: &Function) -> Result<CoroutineFrame, String> {
    let sm = transform_to_state_machine(func)?;

    // 帧大小 = state(1) + params(4*N) + ctx_vars
    let mut frame_size = 1; // state byte
    frame_size += func.params.len() * 4; // 参数保存
    for var in &sm.ctx_vars {
        frame_size += var.size_bytes;
    }
    // 对齐到 4 字节
    frame_size = (frame_size + 3) & !3;

    Ok(CoroutineFrame {
        func_name: func.name.clone(),
        frame_size,
        state_count: sm.state_count,
        zero_alloc: true,  // 状态机始终零动态分配
        realtime_safe: func.is_realtime,
    })
}

/// 生成协程帧报告 (Phase 3.4)
pub fn generate_coroutine_report(frames: &[CoroutineFrame]) -> String {
    let mut report = String::new();
    report.push_str("=== DUANYAN Coroutine Frame Report ===\n\n");
    report.push_str("Function            Frame(B)  States  ZeroAlloc  RT-Safe\n");
    report.push_str("───────────────────────────────────────────────────────────\n");

    let mut total_frame = 0usize;
    for frame in frames {
        report.push_str(&format!(
            "{:<20}{:>6}    {:>4}    {:>7}    {:>5}\n",
            frame.func_name,
            frame.frame_size,
            frame.state_count,
            if frame.zero_alloc { "YES" } else { "NO" },
            if frame.realtime_safe { "YES" } else { "-" },
        ));
        total_frame += frame.frame_size;
    }

    report.push_str(&format!("\nTotal static coroutine RAM: {} bytes\n", total_frame));
    report.push_str("Heap allocation: 0 bytes (stackless coroutines)\n");

    report
}

// === 测试 ===
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn parse_async_fn(code: &str) -> Function {
        let tokens = tokenize(code).unwrap();
        let program = parse(&tokens).unwrap();
        program.functions.into_iter().next().unwrap().func
    }

    #[test]
    fn test_analyze_simple_async() {
        let func = parse_async_fn("async function readSensor() { let val = await ADC.read(0); return val; }");
        let analysis = analyze_async_function(&func).unwrap();
        assert_eq!(analysis.func_name, "readSensor");
        assert_eq!(analysis.await_points.len(), 1);
        assert_eq!(analysis.state_count, 3); // 初始 + 1 await + 完成
    }

    #[test]
    fn test_analyze_multi_await() {
        let func = parse_async_fn(
            "async function process() { let a = await read1(); let b = await read2(); return a + b; }"
        );
        let analysis = analyze_async_function(&func).unwrap();
        assert_eq!(analysis.await_points.len(), 2);
        assert_eq!(analysis.state_count, 4); // 初始 + 2 await + 完成
        // a 在第一个 await 前声明，跨两个 await 存活
        assert!(analysis.live_vars_across_await.iter().any(|v| v.name == "a"));
    }

    #[test]
    fn test_transform_state_machine() {
        let func = parse_async_fn("async function foo() { let x = await bar(); return x; }");
        let sm = transform_to_state_machine(&func).unwrap();
        assert_eq!(sm.name, "foo");
        assert_eq!(sm.state_count, 3);
        assert_eq!(sm.states.len(), 2); // state 0 (await bar) + state 1 (return x)
        assert!(sm.states[0].await_expr.is_some());
        assert!(sm.states[1].is_final);
    }

    #[test]
    fn test_generate_c_state_machine() {
        let func = parse_async_fn("async function readSensor() { let val = await ADC.read(0); return val; }");
        let sm = transform_to_state_machine(&func).unwrap();
        let c_code = generate_c_state_machine(&sm);
        assert!(c_code.contains("typedef struct"));
        assert!(c_code.contains("readSensor_ctx"));
        assert!(c_code.contains("readSensor_poll"));
        assert!(c_code.contains("DUANYAN_PENDING"));
        assert!(c_code.contains("DUANYAN_READY"));
        assert!(c_code.contains("switch (ctx->state)"));
    }

    #[test]
    fn test_generate_c_event_loop_esp32() {
        let code = generate_c_event_loop("esp32");
        assert!(code.contains("vTaskDelay"));
        assert!(code.contains("duanyan_event_loop_run"));
        assert!(code.contains("duanyan_spawn"));
        assert!(code.contains("DUANYAN_PENDING"));
    }

    #[test]
    fn test_generate_c_event_loop_arduino() {
        let code = generate_c_event_loop("arduino");
        assert!(!code.contains("vTaskDelay"));
        assert!(code.contains("duanyan_event_loop_run"));
        assert!(code.contains("裸轮询"));
    }

    #[test]
    fn test_ctx_size_budget() {
        // 验证上下文大小在预算内 (< 64 bytes per async function)
        let func = parse_async_fn(
            "async function multi() { let a = await r1(); let b = await r2(); let c = await r3(); return a; }"
        );
        let analysis = analyze_async_function(&func).unwrap();
        assert!(analysis.ctx_size < 64, "ctx_size={} 超过 64 字节预算", analysis.ctx_size);
    }

    #[test]
    fn test_promise_all_analysis() {
        let func = parse_async_fn(
            "async function fusion() { let r = await Promise.all([a(), b()]); return r; }"
        );
        let analysis = analyze_async_function(&func).unwrap();
        assert_eq!(analysis.await_points.len(), 1);
    }

    // === Phase v1.5: 多模态融合集成测试 ===

    #[test]
    fn test_integration_multimodal_fusion() {
        // 端到端测试: 多传感器异步采集 + AI 推理融合
        let source = r#"
async function senseAndAct() {
    let imu_val = await ADC.read(0);
    let mic_val = await ADC.read(1);
    let gesture = await Brain.classify(imu_val);
    return gesture;
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        
        // 验证解析
        assert_eq!(program.functions.len(), 1);
        let func = &program.functions[0].func;
        assert!(func.is_async);
        assert_eq!(func.name, "senseAndAct");

        // 验证分析
        let analysis = analyze_async_function(func).unwrap();
        assert_eq!(analysis.await_points.len(), 3); // 3 个 await
        assert_eq!(analysis.state_count, 5); // 初始 + 3 await + 完成
        assert!(analysis.ctx_size < 64); // 上下文 < 64B

        // 验证状态机变换
        let sm = transform_to_state_machine(func).unwrap();
        assert_eq!(sm.states.len(), 4); // 4 个状态块

        // 验证 C 代码生成
        let c_code = generate_c_state_machine(&sm);
        assert!(c_code.contains("senseAndAct_ctx"));
        assert!(c_code.contains("senseAndAct_poll"));
        assert!(c_code.contains("DUANYAN_PENDING"));
        assert!(c_code.contains("DUANYAN_READY"));
    }

    #[test]
    fn test_integration_promise_all_multimodal() {
        // Promise.all 多路传感器并发采集
        let source = r#"
async function multiSense() {
    let result = await Promise.all([camera.read(), sensor.read()]);
    return result;
}
"#;
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        let func = &program.functions[0].func;
        
        let analysis = analyze_async_function(func).unwrap();
        assert_eq!(analysis.await_points.len(), 1); // Promise.all 是一个 await 点
        
        let sm = transform_to_state_machine(func).unwrap();
        let c_code = generate_c_state_machine(&sm);
        assert!(c_code.contains("multiSense_ctx"));
        assert!(c_code.contains("Promise.all"));
    }

    #[test]
    fn test_integration_event_loop_esp32() {
        // ESP32 事件循环生成
        let code = generate_c_event_loop("esp32");
        assert!(code.contains("vTaskDelay"));
        assert!(code.contains("duanyan_task_t"));
        assert!(code.contains("duanyan_spawn"));
        assert!(code.contains("duanyan_event_loop_run"));
        assert!(code.contains("DUANYAN_PENDING"));
        assert!(code.contains("DUANYAN_READY"));
        assert!(code.contains("DUANYAN_ERROR"));
    }

    #[test]
    fn test_integration_full_pipeline() {
        // 完整流水线: 源码 → 解析 → 分析 → 变换 → C 代码
        let source = r#"
async function readSensor() {
    let val = await ADC.read(0);
    let result = await Brain.classify(val);
    return result;
}
"#;
        // 1. 解析
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        assert_eq!(program.functions.len(), 1);

        // 2. 分析
        let func = &program.functions[0].func;
        let analysis = analyze_async_function(func).unwrap();
        assert_eq!(analysis.state_count, 4);

        // 3. 变换
        let sm = transform_to_state_machine(func).unwrap();
        assert_eq!(sm.name, "readSensor");

        // 4. C 代码生成
        let c_code = generate_c_state_machine(&sm);
        assert!(c_code.contains("typedef struct"));
        assert!(c_code.contains("readSensor_poll"));
        assert!(c_code.contains("switch (ctx->state)"));
        assert!(c_code.contains("case 0:"));
        assert!(c_code.contains("case 1:"));
        assert!(c_code.contains("case 2:"));

        // 5. 事件循环
        let event_loop = generate_c_event_loop("esp32");
        assert!(event_loop.contains("duanyan_event_loop_run"));
    }

    // Phase 3.4: 无栈协程测试
    #[test]
    fn test_coroutine_frame_size() {
        let tokens = tokenize("async function task(a, b) { let x = await delay(100); return x; }").unwrap();
        let ast = parse(&tokens).unwrap();
        let func = &ast.functions[0].func;

        let frame = analyze_coroutine_frame(func).unwrap();
        assert_eq!(frame.func_name, "task");
        assert!(frame.frame_size > 0, "帧大小应 > 0");
        assert!(frame.zero_alloc, "应零动态分配");
        // state(1) + params(2*4=8) + ctx_vars + alignment
        assert!(frame.frame_size >= 12, "帧应至少包含 state + params");
    }

    #[test]
    fn test_coroutine_report() {
        let tokens = tokenize("async function f1() { await delay(1); } async function f2() { await delay(2); }").unwrap();
        let ast = parse(&tokens).unwrap();

        let frames: Vec<CoroutineFrame> = ast.functions.iter()
            .map(|sf| analyze_coroutine_frame(&sf.func).unwrap())
            .collect();

        let report = generate_coroutine_report(&frames);
        assert!(report.contains("Coroutine Frame Report"));
        assert!(report.contains("f1"));
        assert!(report.contains("f2"));
        assert!(report.contains("stackless coroutines"));
    }

    #[test]
    fn test_coroutine_realtime_safe() {
        let tokens = tokenize("@realtime async function rt_task() { await delay(1); }").unwrap();
        let ast = parse(&tokens).unwrap();
        let func = &ast.functions[0].func;

        let frame = analyze_coroutine_frame(func).unwrap();
        assert!(frame.realtime_safe, "@realtime 协程应标记为 realtime_safe");
    }
}
