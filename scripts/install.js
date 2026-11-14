#!/usr/bin/env node
"use strict";

// postinstall：解析预编译二进制，四级顺序，命中即成功退出。
//
// 铁律：任何失败都必须非零退出。旧版在「缺 cargo」与「构建失败」两种情况下都
// exit 0，导致 npm 显示安装成功、用户却拿到一个不可用的 `duanyan` 命令。
//
// npm tarball 里不含 Rust 源码（见 package.json 的 files 白名单），因此第 4 级
// 只在源码 checkout 中才可能触发——不会再出现「装完偷偷编译 5 分钟」。

const { spawnSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const crypto = require("crypto");
const https = require("https");
const {
  KEYS,
  currentKey,
  isSupported,
  platformPackage,
  binaryName,
  assetName,
  releaseUrl,
} = require("./platforms.js");

const PKG_ROOT = path.join(__dirname, "..");
const VERSION = require("../package.json").version;
const KEY = currentKey();
const TIMEOUT_MS = 30000;
const MAX_REDIRECTS = 5;
const IS_WIN = process.platform === "win32";

// 记录全部尝试过的来源，失败时一并打印，便于 issue 复现
const tried = [];

function log(msg) {
  console.log(msg);
}

function die(msg, hints) {
  console.error("");
  console.error(`ERROR: ${msg}`);
  if (tried.length > 0) {
    console.error("");
    console.error("Tried, in order:");
    for (const item of tried) console.error(`  - ${item}`);
  }
  if (hints && hints.length > 0) {
    console.error("");
    console.error("Next steps:");
    for (const hint of hints) console.error(`  ${hint}`);
  }
  console.error("");
  process.exit(1);
}

function sha256(buf) {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

// 跟随重定向的 HTTPS GET（GitHub Releases 会 302 到 objects.githubusercontent.com）
function httpGet(url, redirectsLeft) {
  return new Promise((resolve, reject) => {
    const req = https.get(url, { timeout: TIMEOUT_MS, headers: { "User-Agent": "duanyan-install" } }, (res) => {
      const status = res.statusCode;
      if (status >= 300 && status < 400 && res.headers.location) {
        res.resume();
        if (redirectsLeft <= 0) {
          reject(new Error(`too many redirects for ${url}`));
          return;
        }
        resolve(httpGet(new URL(res.headers.location, url).toString(), redirectsLeft - 1));
        return;
      }
      if (status !== 200) {
        res.resume();
        reject(new Error(`HTTP ${status} for ${url}`));
        return;
      }
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => resolve(Buffer.concat(chunks)));
      res.on("error", reject);
    });
    req.on("timeout", () => req.destroy(new Error(`timeout after ${TIMEOUT_MS}ms: ${url}`)));
    req.on("error", reject);
  });
}

// 解析 sha256sum 标准输出：`<64位hex>  <文件名>`（兼容二进制模式的 `*` 前缀）
function findChecksum(sumsText, file) {
  for (const line of sumsText.split(/\r?\n/)) {
    const m = line.trim().match(/^([0-9a-fA-F]{64})\s+\*?(.+)$/);
    if (m && m[2].trim() === file) return m[1].toLowerCase();
  }
  return null;
}

function ensureExecutable(file) {
  if (IS_WIN) return;
  try {
    fs.accessSync(file, fs.constants.X_OK);
  } catch {
    fs.chmodSync(file, 0o755);
  }
}

// 第 2 级：平台子包（npm 正常安装的主路径，不触发任何编译或网络）
function tryPlatformPackage() {
  const name = platformPackage(KEY);
  log(`[2/4] platform package ${name} ...`);
  try {
    const resolved = require(name);
    tried.push(`${name} -> ${resolved}`);
    if (typeof resolved === "string" && fs.existsSync(resolved)) {
      ensureExecutable(resolved);
      log(`      found: ${resolved}`);
      return true;
    }
    log("      package resolved but binary is missing");
  } catch (err) {
    tried.push(`${name} (require failed: ${err.code || err.message})`);
    log(`      not installed (${err.code || err.message})`);
  }
  return false;
}

// 第 3 级：从 GitHub Releases 下载并校验 sha256，落到 vendor/
async function tryReleaseDownload() {
  const asset = assetName(KEY);
  const sumsUrl = releaseUrl(VERSION, "SHA256SUMS.txt");
  const binUrl = releaseUrl(VERSION, asset);
  const vendorDir = path.join(PKG_ROOT, "vendor");
  const vendorBin = path.join(vendorDir, binaryName(KEY));

  log(`[3/4] GitHub Releases fallback (v${VERSION}) ...`);
  tried.push(sumsUrl);

  let sumsText;
  try {
    sumsText = (await httpGet(sumsUrl, MAX_REDIRECTS)).toString("utf8");
  } catch (err) {
    log(`      cannot fetch checksums: ${err.message}`);
    return false;
  }

  const expected = findChecksum(sumsText, asset);
  if (!expected) {
    log(`      SHA256SUMS.txt has no entry for ${asset}`);
    return false;
  }

  // 幂等：vendor/ 已有且校验通过则跳过下载
  if (fs.existsSync(vendorBin)) {
    if (sha256(fs.readFileSync(vendorBin)) === expected) {
      tried.push(`${vendorBin} (cached, checksum ok)`);
      ensureExecutable(vendorBin);
      log("      vendor binary already present and verified");
      return true;
    }
    log("      vendor binary checksum mismatch, removing stale copy");
    fs.unlinkSync(vendorBin);
  }

  tried.push(binUrl);
  let buf;
  try {
    buf = await httpGet(binUrl, MAX_REDIRECTS);
  } catch (err) {
    log(`      download failed: ${err.message}`);
    return false;
  }

  // 先校验后落盘，绝不留下未验证的半成品
  const actual = sha256(buf);
  if (actual !== expected) {
    die(`checksum mismatch for ${asset} (expected ${expected}, got ${actual})`, [
      "The release asset may be corrupted or tampered with; do not use it.",
      `Re-download manually and verify: ${binUrl}`,
      "Or build from source: cargo install --path . --locked",
    ]);
  }

  fs.mkdirSync(vendorDir, { recursive: true });
  fs.writeFileSync(vendorBin, buf);
  ensureExecutable(vendorBin);
  log(`      verified sha256 ${actual.slice(0, 12)}... -> ${vendorBin}`);
  return true;
}

// 第 4 级：仅源码 checkout 才允许本地构建（npm tarball 内没有 Cargo.toml）
function trySourceBuild() {
  log("[4/4] source checkout fallback ...");
  const cargoToml = path.join(PKG_ROOT, "Cargo.toml");
  if (!fs.existsSync(cargoToml)) {
    tried.push(`${cargoToml} (absent: not a source checkout)`);
    log("      not a source checkout, skipping local build");
    return false;
  }

  const probe = spawnSync("cargo", ["--version"], { stdio: "pipe", shell: IS_WIN });
  if (probe.error || probe.status !== 0) {
    die("this is a source checkout but Rust/Cargo was not found", [
      "Install Rust stable: https://rustup.rs",
      "Then run: cargo build --release --locked",
    ]);
  }

  log("      building with cargo (release, LTO — this can take several minutes)");
  const build = spawnSync("cargo", ["build", "--release", "--locked"], {
    cwd: PKG_ROOT,
    stdio: "inherit",
    shell: IS_WIN,
  });
  if (build.error || build.status !== 0) {
    die(`cargo build --release --locked failed${build.error ? `: ${build.error.message}` : ` (exit ${build.status})`}`, [
      "Linux requires libudev-dev and pkg-config (serialport dependency).",
      "Fix the build error above, then re-run: npm rebuild @duanyan-lang/duanyan",
    ]);
  }

  const built = path.join(PKG_ROOT, "target", "release", binaryName(KEY));
  tried.push(built);
  if (!fs.existsSync(built)) {
    die(`build succeeded but ${built} is missing`, ["Check the [[bin]] name in Cargo.toml."]);
  }
  ensureExecutable(built);
  log(`      built: ${built}`);
  return true;
}

async function main() {
  log(`[1/4] platform: ${KEY}`);
  if (!isSupported(KEY)) {
    die(`unsupported platform '${KEY}'`, [
      `Supported platforms: ${KEYS.join(", ")}`,
      "Build from source instead:",
      "  git clone https://github.com/duanyan-lang/duanyan.git",
      "  cd duanyan && cargo install --path . --locked",
    ]);
  }

  if (tryPlatformPackage()) {
    log("");
    log("OK: duanyan is ready (prebuilt binary, no Rust toolchain needed).");
    process.exit(0);
  }

  if (await tryReleaseDownload()) {
    log("");
    log("OK: duanyan is ready (downloaded prebuilt binary).");
    process.exit(0);
  }

  if (trySourceBuild()) {
    log("");
    log("OK: duanyan is ready (built from source).");
    process.exit(0);
  }

  die(`could not obtain a duanyan binary for ${KEY}`, [
    "Reinstall so the platform package is fetched:",
    "  npm install -g @duanyan-lang/duanyan",
    `Or download the release asset manually: ${releaseUrl(VERSION, assetName(KEY))}`,
    "Or build from source (requires Rust stable, https://rustup.rs):",
    "  git clone https://github.com/duanyan-lang/duanyan.git",
    "  cd duanyan && cargo install --path . --locked",
  ]);
}

main().catch((err) => {
  die(`unexpected failure: ${err && err.message ? err.message : err}`, [
    "Please report this with the output above:",
    "  https://github.com/duanyan-lang/duanyan/issues",
  ]);
});