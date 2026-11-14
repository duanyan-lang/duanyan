"use strict";

// 平台子包入口：导出 linux-x64 预编译二进制的绝对路径。
// 主包通过 require("@duanyan-lang/duanyan-linux-x64") 取得此路径。
const path = require("path");

module.exports = path.join(__dirname, "bin", "duanyan");