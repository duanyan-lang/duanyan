#!/usr/bin/env node
"use strict";

// duanyan CLI 包装器：解析预编译二进制并透传参数、stdio 与退出码。
// 解析顺序与 scripts/install.js 保持一致，因此 `npm install --ignore-scripts`
// （postinstall 不执行）时本文件仍能自行找到二进制。

const { spawnSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");
const { PLATFORMS, currentKey, platformPackage } = require("../scripts/platforms.js");

const PKG_ROOT = path.join(__dirname, "..");
const KEY = currentKey();
const EXT = PLATFORMS[KEY] ? PLATFORMS[KEY].ext : (process.platform === "win32" ? ".exe" : "");
const BIN_NAME = `duanyan${EXT}`;

// 返回 { binary, tried }；tried 保留全部候选，失败时用于诊断（便于 issue 复现）
function resolveBinary() {
  const tried = [];

  // 1. 平台子包：其 index.js 导出二进制绝对路径，这是 npm 正常安装的主路径
  const subPkg = platformPackage(KEY);
  try {
    const resolved = require(subPkg);
    tried.push(`${subPkg} -> ${resolved}`);
    if (typeof resolved === "string" && fs.existsSync(resolved)) {
      return { binary: resolved, tried };
    }
  } catch (err) {
    tried.push(`${subPkg} (require failed: ${err.code || err.message})`);
  }

  // 2. postinstall 从 GitHub Releases 下载的兜底产物
  const vendor = path.join(PKG_ROOT, "vendor", BIN_NAME);
  tried.push(vendor);
  if (fs.existsSync(vendor)) return { binary: vendor, tried };

  // 3. 源码 checkout 的构建产物
  const localRelease = path.join(PKG_ROOT, "target", "release", BIN_NAME);
  tried.push(localRelease);
  if (fs.existsSync(localRelease)) return { binary: localRelease, tried };

  // 4. cargo install --path . 的安装位置
  const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
  const cargoBin = path.join(cargoHome, "bin", BIN_NAME);
  tried.push(cargoBin);
  if (fs.existsSync(cargoBin)) return { binary: cargoBin, tried };

  // 5. 交给 PATH 解析；找不到时 spawnSync 会给出 ENOENT
  tried.push(`${BIN_NAME} (PATH)`);
  return { binary: BIN_NAME, tried };
}

function reportMissing(tried) {
  console.error(`Error: cannot find the 'duanyan' binary for ${KEY}.`);
  console.error("");
  console.error("Tried, in order:");
  for (const item of tried) console.error(`  - ${item}`);
  console.error("");
  console.error("Fix by reinstalling so the platform package is present:");
  console.error("  npm install -g @duanyan-lang/duanyan");
  console.error("");
  console.error("Or build from source (requires Rust stable, https://rustup.rs):");
  console.error("  git clone https://github.com/duanyan-lang/duanyan.git");
  console.error("  cd duanyan && cargo install --path . --locked");
}

const { binary, tried } = resolveBinary();
const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

// spawn 失败与子进程非零退出是两回事：旧版只要 err.status 为 null 就一律报
// 「找不到二进制」，把 EACCES 等真实错误也伪装成缺文件，这里区分处理。
if (result.error) {
  if (result.error.code === "ENOENT") {
    reportMissing(tried);
  } else {
    console.error(`Error: failed to start '${binary}' (${result.error.code}): ${result.error.message}`);
  }
  process.exit(1);
}

// 透传信号：以相同信号结束自身，让上层 shell 看到一致的退出语义
if (result.signal) {
  process.kill(process.pid, result.signal);
  process.exit(1); // 兜底，绝不因信号而退出 0
}

process.exit(result.status === null ? 1 : result.status);