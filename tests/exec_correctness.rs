//! 编译正确性回归测试
//!
//! 这些用例来自《HardyScript 开发教程》验证阶段实际发现的错误。
//! 每一个曾经静默生成错误产物或报出寄存器耗尽，
//! 修复后必须保持通过，不允许再退化。

use duanyan::{compile_source, compile_source_optimized, engine::Engine};

fn run_source(source: &str) -> Result<String, String> {
    let code = compile_source(source)?;
    let mut eng = Engine::new(code);
    let (out, _) = eng.run()?;
    Ok(out)
}

fn run_source_optimized(source: &str) -> Result<String, String> {
    let code = compile_source_optimized(source)?;
    let mut eng = Engine::new(code);
    let (out, _) = eng.run()?;
    Ok(out)
}

/// 断言普通模式与 -O 模式输出完全一致，且等于期望文本。
fn assert_both_modes(source: &str, expected: &str) {
    let plain = run_source(source).unwrap_or_else(|e| panic!("普通模式编译/执行失败: {}", e));
    let opt = run_source_optimized(source).unwrap_or_else(|e| panic!("优化模式编译/执行失败: {}", e));
    assert_eq!(plain, expected, "普通模式输出不匹配");
    assert_eq!(opt, expected, "优化模式输出不匹配");
}

// ---------------------------------------------------------------------------
// 用例 1：函数中间 return 必须立即退出函数
// ---------------------------------------------------------------------------

#[test]
fn regression_clamp_early_return() {
    let source = r#"
function clamp(value, low, high) {
    if (value < low) {
        return low;
    }
    if (value > high) {
        return high;
    }
    return value;
}
let safe = clamp(120, 0, 100);
console.log(safe);
"#;
    assert_both_modes(source, "100\n");
}

#[test]
fn regression_early_return_first_branch_wins() {
    // 第一个分支命中后，后面的 return 不能覆盖结果
    let source = r#"
function pick(x) {
    if (x < 10) {
        return 1;
    }
    if (x < 20) {
        return 2;
    }
    return 3;
}
console.log(pick(5));
console.log(pick(15));
console.log(pick(50));
"#;
    assert_both_modes(source, "1\n2\n3\n");
}

// ---------------------------------------------------------------------------
// 用例 2：分支内声明的变量不能泄漏/冲突，作用域按块处理
// ---------------------------------------------------------------------------

#[test]
fn regression_block_scoped_if_branch_vars() {
    // 两个分支各自声明同名局部变量，互不干扰
    let source = r#"
let flag = 1;
if (flag === 1) {
    let t = 7;
    console.log(t);
} else {
    let t = 9;
    console.log(t);
}
"#;
    assert_both_modes(source, "7\n");
}

// ---------------------------------------------------------------------------
// 用例 3：&& / || 在条件里必须是标准短路语义
// ---------------------------------------------------------------------------

#[test]
fn regression_logical_and_in_if() {
    let source = r#"
const LIMIT = 100;
let value = 25;
let enabled = true;
if (enabled && value < LIMIT) {
    console.log(value);
}
"#;
    assert_both_modes(source, "25\n");
}

#[test]
fn regression_logical_or_in_if() {
    let source = r#"
let a = 0;
let b = 5;
if (a || b) {
    console.log(b);
}
"#;
    assert_both_modes(source, "5\n");
}

#[test]
fn regression_logical_and_short_circuit_false() {
    let source = r#"
let a = 0;
let b = 5;
if (a && b) {
    console.log(b);
}
console.log(a);
"#;
    assert_both_modes(source, "0\n");
}

// ---------------------------------------------------------------------------
// 用例 4：大整数常量不能被 12 位立即数静默截断
// ---------------------------------------------------------------------------

#[test]
fn regression_large_const_no_truncation() {
    let source = r#"
let baud = 115200;
console.log(baud);
"#;
    assert_both_modes(source, "115200\n");
}

#[test]
fn regression_large_const_in_condition() {
    let source = r#"
let raw = 3000;
if (raw > 2200) {
    console.log(raw);
}
"#;
    assert_both_modes(source, "3000\n");
}

// ---------------------------------------------------------------------------
// 用例 5：状态机（回差）示例——复杂控制流不能退化为恒定输出
// ---------------------------------------------------------------------------

#[test]
fn regression_hysteresis_state_machine() {
    let source = r#"
let state = 0;
for (let sample = 0; sample < 5; sample++) {
    let raw = 1800;
    if (sample === 1) {
        raw = 1300;
    }
    if (sample === 3) {
        raw = 2300;
    }
    if (state === 0) {
        if (raw < 1400) {
            state = 1;
        }
    } else {
        if (raw > 2200) {
            state = 0;
        }
    }
    console.log(state);
}
"#;
    assert_both_modes(source, "0\n1\n1\n0\n0\n");
}

// ---------------------------------------------------------------------------
// 用例 6：数组循环不能在寄存器分配阶段直接耗尽
// ---------------------------------------------------------------------------

#[test]
fn regression_array_average_does_not_exhaust_registers() {
    let source = r#"
let readings = [100, 200, 300, 400];
let sum = 0;
for (let i = 0; i < 4; i++) {
    sum = sum + readings[i];
}
console.log(sum / 4);
"#;
    assert_both_modes(source, "250\n");
}

// ---------------------------------------------------------------------------
// 用例 7：函数调用不能破坏调用者活跃变量（寄存器保存）
// ---------------------------------------------------------------------------

#[test]
fn regression_call_preserves_caller_state() {
    let source = r#"
function add(a, b) {
    return a + b;
}
let x = 10;
let y = 20;
let z = add(x, y);
console.log(x);
console.log(z);
"#;
    assert_both_modes(source, "10\n30\n");
}

// ---------------------------------------------------------------------------
// 用例 8：语言完备性批 2 —— continue / break 控制流
// ---------------------------------------------------------------------------

#[test]
fn continue_skips_to_update_in_for_loop() {
    // 1..6 中跳过 3 的倍数：1+2+4+5 = 12
    let source = r#"
let sum = 0;
for (let i = 1; i <= 6; i += 1) {
    if (i % 3 == 0) { continue; }
    sum += i;
}
console.log(sum);
"#;
    assert_both_modes(source, "12\n");
}

#[test]
fn break_exits_for_loop() {
    let source = r#"
let count = 0;
for (let i = 0; i < 10; i++) {
    if (i == 5) { break; }
    count++;
}
console.log(count);
"#;
    assert_both_modes(source, "5\n");
}

#[test]
fn continue_and_break_in_while_loop() {
    // guard 1..10 中只有奇数计数：n = 5
    let source = r#"
let n = 0;
let guard = 0;
while (guard < 10) {
    guard++;
    if (guard % 2 == 0) { continue; }
    n += 1;
    if (guard > 20) { break; }
}
console.log(n);
"#;
    assert_both_modes(source, "5\n");
}

#[test]
fn continue_inside_switch_targets_loop() {
    // JS 语义：switch 里的 continue 作用于外层循环。跳过偶数：1+3 = 4
    let source = r#"
let s = 0;
for (let i = 1; i <= 4; i++) {
    switch (i % 2) {
        case 0: continue;
        default: s += i;
    }
}
console.log(s);
"#;
    assert_both_modes(source, "4\n");
}

#[test]
fn break_inside_switch_does_not_exit_loop() {
    // JS 语义：switch 里的 break 只退出 switch，循环继续。
    // i=0: +0+10; i=1: break 后仍 +10; i=2: +2+10; i=3: +3+10 → 45
    let source = r#"
let total = 0;
for (let i = 0; i < 4; i++) {
    switch (i) {
        case 1: break;
        default: total += i;
    }
    total += 10;
}
console.log(total);
"#;
    assert_both_modes(source, "45\n");
}

#[test]
fn nested_loops_bind_to_innermost() {
    // 内层 continue 不影响外层；内层 break 只退出内层。
    // 每轮内层：j=0 累加、j=1/3 跳过、j=2 累加、j=4 断开 → 每轮 +2，外层 3 轮 → 6
    let source = r#"
let acc = 0;
for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 10; j++) {
        if (j % 2 == 1) { continue; }
        if (j == 4) { break; }
        acc += j;
    }
}
console.log(acc);
"#;
    assert_both_modes(source, "6\n");
}

#[test]
fn continue_outside_loop_is_error() {
    let source = r#"
let a = 1;
continue;
"#;
    assert!(run_source(source).is_err(), "循环外的 continue 应报错");
}

// ---------------------------------------------------------------------------
// 用例 9：定点小数字面量（统一 ×100 缩放）
// ---------------------------------------------------------------------------

#[test]
fn fixed_point_literals_scale_by_100() {
    let source = r#"
let ratio = 1.5;
let bonus = 2.25;
console.log(ratio + bonus);
let unit_price = 0.99;
let qty = 3;
console.log(unit_price * qty);
let avg = (1.5 + 2.5) / 2;
console.log(avg);
"#;
    // 1.50+2.25=3.75→375；0.99×3=2.97→297；(1.5+2.5)/2=2.00→200
    assert_both_modes(source, "375\n297\n200\n");
}

#[test]
fn fixed_point_three_digits_is_error() {
    let source = r#"
let x = 1.234;
console.log(x);
"#;
    assert!(run_source(source).is_err(), "3 位小数应被词法器拒绝");
}

// ---------------------------------------------------------------------------
// 用例 10：Math 扩展脱糖（floor/round 恒等，sign 脱糖）
// ---------------------------------------------------------------------------

#[test]
fn math_floor_round_identity_and_sign_desugar() {
    let source = r#"
let a = Math.floor(7);
let b = Math.round(2);
let v = 0 - 5;
let s1 = Math.sign(v);
let s2 = Math.sign(9);
let s3 = Math.sign(0);
console.log(a, b, s1, s2, s3);
"#;
    assert_both_modes(source, "7\n2\n-1\n1\n0\n");
}

#[test]
fn math_sqrt_rejected_on_native_engine() {
    // COUNPRE64 本机引擎不提供 sqrt/pow（需要循环），应明确报错而非静默错算
    assert!(run_source("let r = Math.sqrt(16);").is_err());
    assert!(run_source("let p = Math.pow(2, 3);").is_err());
}

// ---------------------------------------------------------------------------
// 用例 11：位运算族 & | ^ ~ << >> >>>（债1清偿回归锁）
//
// 语义基准：64 位 i64。移位量按 & 63 掩码；>> 为算术右移（保号）；
// >>> 为 i64 无符号右移（与 JS 的 32 位 ToUint32 口径不同，已文档化）。
// ---------------------------------------------------------------------------

#[test]
fn bitwise_operators_basic_values() {
    let source = r#"
console.log(12 & 10);
console.log(12 | 10);
console.log(12 ^ 10);
console.log(~5);
console.log(~0);
console.log(3 << 4);
console.log(256 >> 3);
console.log(-8 >> 1);
console.log(-8 >>> 60);
"#;
    // -8 >> 1 = -4（算术保号）；-8 >>> 60 = 0xF（i64 无符号）
    assert_both_modes(source, "8\n14\n6\n-6\n-1\n48\n32\n-4\n15\n");
}

#[test]
fn bitwise_precedence_matches_js() {
    let source = r#"
console.log(1 | 2 & 3);
console.log(1 ^ 3 & 1);
console.log(1 + 2 << 3);
console.log(1 << 2 < 8);
console.log(~2 + 1);
"#;
    // & > ^ > |；移位低于关系比较但高于加减；一元 ~ 低于算术
    assert_both_modes(source, "3\n0\n24\n1\n-2\n");
}

#[test]
fn bitwise_in_conditions_and_variables() {
    let source = r#"
let flags = 12;
let mask = 10;
let masked = flags & mask;
if (masked == 8) {
    console.log(1);
} else {
    console.log(0);
}
if (flags & 1) {
    console.log(99);
} else {
    console.log(100);
}
let n = 5;
console.log(~n);
"#;
    // 位运算结果非 0 即真（规范化 0/1）；变量路径与常量路径同口径
    assert_both_modes(source, "1\n100\n-6\n");
}

#[test]
fn bitwise_compound_assign_gives_friendly_error() {
    // &= |= <<= >>= 尚未实现，必须友好报错而非词法崩溃
    let e = run_source("let x = 1; x &= 3;").unwrap_err();
    assert!(e.contains("&="), "应提示 '&=' 尚未实现，实际: {}", e);
}

// ---------------------------------------------------------------------------
// 用例 12：async/await 字节码真悬停（债2清偿回归锁）
//
// 语义：async 函数调用 = SPAWN 派生任务；await Timer.delay(ms) = YIELD_T
// 挂起到虚拟时钟；主程序 HALT 后由协作式调度器驱动至全部任务 TRET。
// ---------------------------------------------------------------------------

fn run_engine(source: &str) -> Result<(String, Engine), String> {
    let code = compile_source(source)?;
    let mut eng = Engine::new(code);
    let (out, _) = eng.run()?;
    Ok((out, eng))
}

#[test]
fn async_tasks_interleave_by_virtual_time() {
    let source = r#"
async function taskA() {
    console.log("A1");
    await Timer.delay(10);
    console.log("A2");
}
async function taskB() {
    await Timer.delay(5);
    console.log("B");
}
console.log("M");
taskA();
taskB();
console.log("E");
"#;
    // 主程序先跑完（M/E），再轮转任务：A1(t=0) → B 在 t=5 先醒 → A2 在 t=10
    let (out, eng) = run_engine(source).unwrap();
    assert_eq!(out, "M\nE\nA1\nB\nA2\n");
    assert_eq!(eng.vtime(), 10, "虚拟时钟应停在最后唤醒点");
    assert_eq!(eng.active_task_count(), 0, "全部任务应已 TRET 结束");
}

#[test]
fn await_delay_records_real_suspension() {
    let source = r#"
async function worker() {
    let v = await Timer.delay(7);
    console.log(v);
}
worker();
"#;
    let (out, eng) = run_engine(source).unwrap();
    assert_eq!(out, "0\n", "await Timer.delay 求值为 0");
    // suspend_log: (task_id, 挂起时刻, 时长)；taskA=1 在 t=0 挂起 7ms
    assert_eq!(eng.suspend_log(), &[(1, 0, 7)], "await 必须留下真悬停记录");
    assert_eq!(eng.vtime(), 7);
}

#[test]
fn async_main_order_stays_deterministic() {
    // 主程序不与任务并发：SPAWN 只登记，HALT 后任务才开始跑（确定性口径）
    let source = r#"
async function bg() {
    console.log("BG");
}
bg();
console.log("MAIN");
"#;
    let (out, _eng) = run_engine(source).unwrap();
    assert_eq!(out, "MAIN\nBG\n");
}

#[test]
fn async_tasks_stack_isolated_across_await() {
    // 回归锁：两个 async 任务各持跨 await 的活跃局部变量，
    // 验证静态栈池按槽隔离——挂起任务的栈帧不得被后启动任务覆盖。
    // （曾因所有任务共享 STACK_INIT 导致 a=111 被覆写成 222）
    let source = r#"
async function p() {
    let a = 111;
    await Timer.delay(10);
    console.log(a);
}
async function q() {
    let a = 222;
    await Timer.delay(5);
    console.log(a);
}
p();
q();
"#;
    // 正确语义：q 在 t=5 先醒打印 222，p 在 t=10 打印 111
    let (out, _eng) = run_engine(source).unwrap();
    assert_eq!(out, "222\n111\n", "跨 await 局部变量应保持任务隔离");
}
