#!/usr/bin/env node
"use strict";

// 平台矩阵一致性闸门（CI 与本地共用）。
// 矩阵分散在 5 处，任何一处漂移都会导致「某平台装不上」或「子包版本对不上」：
//   1. scripts/platforms.js                （单一真值）
//   2. package.json 的 optionalDependencies
//   3. npm/<key>/package.json 的 name/os/cpu/version
//   4. .github/workflows/release.yml 的 matrix（target 三元组 + runner 标签）
//   5. Cargo.toml 与 package.json 的版本
// 本脚本逐项核对，任一不符即非零退出。

const fs = require("fs");
const path = require("path");
const {
  PLATFORMS,
  KEYS,
  platformPackage,
  binaryName,
} = require("./platforms.js");

const ROOT = path.join(__dirname, "..");
const errors = [];

function err(msg) {
  errors.push(msg);
}

function readJson(rel) {
  return JSON.parse(fs.readFileSync(path.join(ROOT, rel), "utf8"));
}

// 1. 版本单一真值：Cargo.toml == package.json
const cargoText = fs.readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
const cargoMatch = cargoText.match(/^\s*version\s*=\s*"([^"]+)"/m);
if (!cargoMatch) err("Cargo.toml: cannot find [package] version");
const cargoVersion = cargoMatch ? cargoMatch[1] : null;

const mainPkg = readJson("package.json");
if (mainPkg.version !== cargoVersion) {
  err(`version mismatch: Cargo.toml=${cargoVersion} package.json=${mainPkg.version}`);
}

// 2. optionalDependencies 必须与矩阵完全一致（不多不少，版本相同）
const optDeps = mainPkg.optionalDependencies || {};
const expectedNames = KEYS.map(platformPackage);
for (const name of expectedNames) {
  if (!Object.prototype.hasOwnProperty.call(optDeps, name)) {
    err(`package.json optionalDependencies missing ${name}`);
  } else if (optDeps[name] !== cargoVersion) {
    err(`package.json optionalDependencies ${name}=${optDeps[name]}, expected ${cargoVersion}`);
  }
}
for (const name of Object.keys(optDeps)) {
  if (!expectedNames.includes(name)) {
    err(`package.json optionalDependencies has unexpected entry ${name}`);
  }
}

// 3. files 白名单必须包含运行期需要的全部文件，且不得包含源码/二进制
const files = mainPkg.files || [];
for (const required of ["bin/duanyan.js", "scripts/install.js", "scripts/platforms.js"]) {
  if (!files.includes(required)) err(`package.json files missing ${required}`);
}
if (files.some((f) => f.startsWith("src"))) {
  err("package.json files must not ship Rust sources (src/)");
}

// 4. 每个平台子包模板
for (const key of KEYS) {
  const p = PLATFORMS[key];
  const dir = path.join(ROOT, "npm", key);
  const pkgPath = path.join(dir, "package.json");
  const indexPath = path.join(dir, "index.js");

  if (!fs.existsSync(pkgPath)) { err(`npm/${key}/package.json missing`); continue; }
  if (!fs.existsSync(indexPath)) { err(`npm/${key}/index.js missing`); continue; }

  const sub = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  if (sub.name !== platformPackage(key)) err(`npm/${key}: name=${sub.name}, expected ${platformPackage(key)}`);
  if (sub.version !== cargoVersion) err(`npm/${key}: version=${sub.version}, expected ${cargoVersion}`);
  if (JSON.stringify(sub.os) !== JSON.stringify([p.os])) err(`npm/${key}: os=${JSON.stringify(sub.os)}, expected ["${p.os}"]`);
  if (JSON.stringify(sub.cpu) !== JSON.stringify([p.cpu])) err(`npm/${key}: cpu=${JSON.stringify(sub.cpu)}, expected ["${p.cpu}"]`);
  if (sub.main !== "index.js") err(`npm/${key}: main must be index.js`);
  if (sub.bin) err(`npm/${key}: platform packages must not declare a bin entry`);

  // index.js 必须导出与平台匹配的二进制名
  const indexText = fs.readFileSync(indexPath, "utf8");
  if (!indexText.includes(JSON.stringify(binaryName(key)))) {
    err(`npm/${key}/index.js must export binary named ${binaryName(key)}`);
  }
}

// 5. release.yml 的 matrix 必须覆盖每个 target 与 runner（纯文本核对，不引 YAML 依赖）
const releasePath = path.join(ROOT, ".github", "workflows", "release.yml");
if (!fs.existsSync(releasePath)) {
  err(".github/workflows/release.yml missing");
} else {
  const yml = fs.readFileSync(releasePath, "utf8");
  for (const key of KEYS) {
    const p = PLATFORMS[key];
    if (!yml.includes(`platform: ${key}`)) err(`release.yml matrix missing platform ${key}`);
    if (!yml.includes(p.target)) err(`release.yml matrix missing target ${p.target} (${key})`);
    if (!yml.includes(p.runner)) err(`release.yml matrix missing runner ${p.runner} (${key})`);
  }
  // 退役 runner 标签黑名单：用了会无限排队而非报错
  for (const banned of ["macos-13", "macos-14", "ubuntu-22.04", "ubuntu-latest", "macos-latest", "windows-latest"]) {
    if (yml.includes(banned)) err(`release.yml uses deprecated/unpinned runner label '${banned}'`);
  }
}

if (errors.length > 0) {
  console.error(`Platform sync check FAILED (${errors.length} issue(s)):`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}

console.log(`Platform sync check OK: ${KEYS.length} platforms, version ${cargoVersion}`);
for (const key of KEYS) {
  console.log(`  ${platformPackage(key)}  ${PLATFORMS[key].target}  @${PLATFORMS[key].runner}`);
}