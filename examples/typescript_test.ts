// TypeScript 端到端测试
interface Config {
    value: number;
}

type ID = number;

function add(a: number, b: number): number {
    return a + b;
}

let x: number = add(3, 7);
let y: number = x * 2;
console.log(y);
