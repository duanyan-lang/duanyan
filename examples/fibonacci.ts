// Phase 5 TypeScript 测试
// HardyScript 自动剥离类型注解后编译

interface Config {
    speed: number;
    debug: boolean;
}

type Result = number;

function fibonacci(n: number): number {
    let a: number = 0;
    let b: number = 1;
    let i: number = 0;
    while (i < n) {
        let temp: number = b;
        b = a + b;
        a = temp;
        i = i + 1;
    }
    return a;
}

let result: number = fibonacci(10);
console.log(result);
