// Phase A 特性测试

// A.5: const 声明
const MAX = 100;
const PI_APPROX = 3;

// A.5: let + const 混用
let x = 10;
let y = 20;
let sum = x + y;
console.log(sum);

// A.7: 三元运算符
let result = x > y ? 1 : 0;
console.log(result);

// A: true / false / null
let flag = true;
let empty = null;
let off = false;
console.log(flag);

// A.3: switch-case
let code = 2;
switch (code) {
    case 1:
        console.log(100);
        break;
    case 2:
        console.log(200);
        break;
    case 3:
        console.log(300);
        break;
    default:
        console.log(999);
        break;
}

// A: === 和 !== 严格相等
let a = 5;
let b = 5;
let strict_eq = a === b;
console.log(strict_eq);

// A.1: 箭头函数
let add = (a, b) => a + b;
let square = (n) => { return n * n; };

// A.2: 模板字符串
let name = "world";
let greeting = `Hello ${name}!`;

// A.6: 解构赋值
let point = { px: 10, py: 20 };
let { px, py } = point;
console.log(px);
console.log(py);

// A.4: import 示例
// import { GPIO, UART } from "snar:hw";
