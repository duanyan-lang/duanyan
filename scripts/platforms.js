"use strict";

// 平台矩阵的单一真值：bin/duanyan.js、scripts/install.js、scripts/build-platform-package.js 共用。
// 新增/调整平台时只改这里，再同步 .github/workflows/release.yml 的 matrix。
//
// key 一律等于 `${process.platform}-${process.arch}`，与 npm 子包名后缀一致，
// 这样 npm 会依据子包自身的 os/cpu 字段自动只安装当前平台那一个。

const SCOPE = "@duanyan-lang";
const MAIN_PKG = `${SCOPE}/duanyan`;
const REPO = "duanyan-lang/duanyan";

// ext: 二进制扩展名；target: Rust 目标三元组；runner: GitHub Actions 标签
// runner 标签已避开退役镜像：macos-13(2025-12 退役)、macos-14(2026-11 移除)、
// ubuntu-22.04(2026-09 起弃用)；也不使用 -latest（会随镜像切换而漂移）。
const PLATFORMS = {
  "win32-x64": {
    os: "win32", cpu: "x64", ext: ".exe",
    target: "x86_64-pc-windows-msvc", runner: "windows-2025",
  },
  "win32-arm64": {
    os: "win32", cpu: "arm64", ext: ".exe",
    target: "aarch64-pc-windows-msvc", runner: "windows-2025",
  },
  "linux-x64": {
    os: "linux", cpu: "x64", ext: "",
    target: "x86_64-unknown-linux-gnu", runner: "ubuntu-24.04",
  },
  "linux-arm64": {
    os: "linux", cpu: "arm64", ext: "",
    target: "aarch64-unknown-linux-gnu", runner: "ubuntu-24.04-arm",
  },
  "darwin-x64": {
    os: "darwin", cpu: "x64", ext: "",
    target: "x86_64-apple-darwin", runner: "macos-15-intel",
  },
  "darwin-arm64": {
    os: "darwin", cpu: "arm64", ext: "",
    target: "aarch64-apple-darwin", runner: "macos-15",
  },
};

const KEYS = Object.keys(PLATFORMS);

function currentKey() {
  return `${process.platform}-${process.arch}`;
}

function isSupported(key) {
  return Object.prototype.hasOwnProperty.call(PLATFORMS, key);
}

// 平台子包名，例如 @duanyan-lang/duanyan-win32-x64
function platformPackage(key) {
  return `${SCOPE}/duanyan-${key}`;
}

// 包内二进制文件名，例如 duanyan.exe
function binaryName(key) {
  return `duanyan${PLATFORMS[key].ext}`;
}

// GitHub Release 资产名，例如 duanyan-win32-x64.exe
function assetName(key) {
  return `duanyan-${key}${PLATFORMS[key].ext}`;
}

function releaseUrl(version, file) {
  return `https://github.com/${REPO}/releases/download/v${version}/${file}`;
}

// Linux 构建需要 libudev（Cargo.lock 里的 libudev-sys 来自 serialport），
// 供 CI 与本地文档共用同一口径。
const LINUX_BUILD_DEPS = ["libudev-dev", "pkg-config"];

module.exports = {
  SCOPE,
  MAIN_PKG,
  REPO,
  PLATFORMS,
  KEYS,
  LINUX_BUILD_DEPS,
  currentKey,
  isSupported,
  platformPackage,
  binaryName,
  assetName,
  releaseUrl,
};