#!/usr/bin/env node
"use strict";

// 组装平台 npm 子包（本地与 CI 共用同一脚本，避免两条口径漂移）。
//
// 用法:
//   node scripts/build-platform-package.js                          # 当前平台，缺二进制则先 cargo build
//   node scripts/build-platform-package.js --no-build               # 只用已有二进制，不触发构建
//   node scripts/build-platform-package.js --platform win32-arm64 --target aarch64-pc-windows-msvc
//   node scripts/build-platform-package.js --binary <path>          # 直接指定二进制（CI 下载 artifact 后）
//   node scripts/build-platform-package.js --platform linux-x64 --from-dist --no-build
//
// 产出:
//   npm/<key>/     可 npm publish 的子包（package.json/index.js/bin/README.md/LICENSE）
//   dist/<asset>   GitHub Release 资产副本 + dist/SHA256SUMS.txt（追加）
//
// 版本单一真值是 Cargo.toml；本脚本会校验 package.json 与之一致，并强制写入子包版本。

const { spawnSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const crypto = require("crypto");
const {
  PLATFORMS,
  KEYS,
  currentKey,
  isSupported,
  platformPackage,
  binaryName,
  assetName,
} = require("./platforms.js");

const ROOT = path.join(__dirname, "..");
const NPM_DIR = path.join(ROOT, "npm");
const DIST_DIR = path.join(ROOT, "dist");
const IS_WIN = process.platform === "win32";

function usage() {
  console.log("Usage: node scripts/build-platform-package.js [--platform <key>] [--target <triple>]");
  console.log("                                             [--binary <path>] [--from-dist] [--no-build]");
  console.log(`Platforms: ${KEYS.join(", ")}`);
}

function fail(msg) {
  console.error(`ERROR: ${msg}`);
  process.exit(1);
}

function parseArgs(argv) {
  const opts = { platform: currentKey(), target: null, binary: null, build: true, fromDist: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--platform") opts.platform = argv[++i];
    else if (arg === "--target") opts.target = argv[++i];
    else if (arg === "--binary") opts.binary = argv[++i];
    else if (arg === "--from-dist") opts.fromDist = true;
    else if (arg === "--no-build") opts.build = false;
    else if (arg === "--help" || arg === "-h") { usage(); process.exit(0); }
    else { console.error(`Unknown argument: ${arg}`); usage(); process.exit(1); }
  }
  return opts;
}

// Cargo.toml 的 [package] version 是版本单一真值（main.rs 用 env!("CARGO_PKG_VERSION")）
function cargoVersion() {
  const text = fs.readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  const m = text.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!m) fail("cannot read [package] version from Cargo.toml");
  return m[1];
}

function sha256(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function defaultPackageJson(key, version) {
  const p = PLATFORMS[key];
  return {
    name: platformPackage(key),
    version,
    description: `Prebuilt duanyan binary for ${key} (internal platform package of @duanyan-lang/duanyan)`,
    license: "MIT",
    repository: {
      type: "git",
      url: "https://github.com/duanyan-lang/duanyan.git",
      directory: `npm/${key}`,
    },
    os: [p.os],
    cpu: [p.cpu],
    main: "index.js",
    files: ["bin", "index.js", "README.md", "LICENSE"],
    publishConfig: { access: "public" },
  };
}

function indexJsSource(key) {
  return [
    '"use strict";',
    "",
    `// 平台子包入口：导出 ${key} 预编译二进制的绝对路径。`,
    `// 主包通过 require("${platformPackage(key)}") 取得此路径。`,
    'const path = require("path");',
    "",
    `module.exports = path.join(__dirname, "bin", ${JSON.stringify(binaryName(key))});`,
    "",
  ].join("\n");
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const key = opts.platform;

  if (!isSupported(key)) {
    fail(`unsupported platform '${key}'. Supported: ${KEYS.join(", ")}`);
  }
  const platform = PLATFORMS[key];
  const target = opts.target || (key === currentKey() ? null : platform.target);
  const binName = binaryName(key);
  const version = cargoVersion();

  // 版本一致性闸门：package.json 必须与 Cargo.toml 同步
  const pkgVersion = require(path.join(ROOT, "package.json")).version;
  if (pkgVersion !== version) {
    fail(`version mismatch: package.json=${pkgVersion} but Cargo.toml=${version}. Sync them first.`);
  }

  // 定位二进制：显式路径 > dist/ 内的 Release 资产 > cargo 构建产物
  let srcBin;
  if (opts.binary) srcBin = path.resolve(opts.binary);
  else if (opts.fromDist) srcBin = path.join(DIST_DIR, assetName(key));
  else srcBin = path.join(ROOT, "target", ...(target ? [target] : []), "release", binName);

  if (!fs.existsSync(srcBin)) {
    if (!opts.build) fail(`binary not found: ${srcBin} (and --no-build was given)`);
    if (key !== currentKey() && !opts.target) {
      fail(`cross-building for ${key} requires --target ${platform.target}`);
    }
    console.log(`Building ${key} (${target || "host target"}) ...`);
    const args = ["build", "--release", "--locked"];
    if (target) args.push("--target", target);
    const build = spawnSync("cargo", args, { cwd: ROOT, stdio: "inherit", shell: IS_WIN });
    if (build.error || build.status !== 0) {
      fail(`cargo build failed${build.error ? `: ${build.error.message}` : ` (exit ${build.status})`}`);
    }
    if (!fs.existsSync(srcBin)) fail(`build finished but ${srcBin} is missing`);
  }

  // 组装子包
  const dir = path.join(NPM_DIR, key);
  const binDir = path.join(dir, "bin");
  fs.mkdirSync(binDir, { recursive: true });
  const destBin = path.join(binDir, binName);
  fs.copyFileSync(srcBin, destBin);
  if (!IS_WIN) fs.chmodSync(destBin, 0o755);

  // package.json：保留已有模板字段，强制同步版本
  const pkgPath = path.join(dir, "package.json");
  const pkg = fs.existsSync(pkgPath)
    ? JSON.parse(fs.readFileSync(pkgPath, "utf8"))
    : defaultPackageJson(key, version);
  pkg.name = platformPackage(key);
  pkg.version = version;
  pkg.os = [platform.os];
  pkg.cpu = [platform.cpu];
  fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);

  // index.js：保留已有模板，缺失才生成
  const indexPath = path.join(dir, "index.js");
  if (!fs.existsSync(indexPath)) fs.writeFileSync(indexPath, indexJsSource(key));

  // README / LICENSE 由共享模板与根 LICENSE 复制（属产物，不入库）
  const sharedReadme = path.join(NPM_DIR, "README.md");
  if (fs.existsSync(sharedReadme)) fs.copyFileSync(sharedReadme, path.join(dir, "README.md"));
  fs.copyFileSync(path.join(ROOT, "LICENSE"), path.join(dir, "LICENSE"));

  // Release 资产副本 + 校验和
  fs.mkdirSync(DIST_DIR, { recursive: true });
  const asset = assetName(key);
  fs.copyFileSync(destBin, path.join(DIST_DIR, asset));
  const hash = sha256(destBin);
  // 幂等：同一资产重复组装时替换旧行，而不是不断追加
  // （publish-npm.yml 会先下载 Release 的 SHA256SUMS.txt 再逐个组装 6 个平台）
  const sumsPath = path.join(DIST_DIR, "SHA256SUMS.txt");
  const kept = fs.existsSync(sumsPath)
    ? fs.readFileSync(sumsPath, "utf8").split(/\r?\n/).filter((line) => {
        const m = line.match(/^[0-9a-fA-F]{64}\s+\*?(.+)$/);
        return m && m[1].trim() !== asset;
      })
    : [];
  kept.push(`${hash}  ${asset}`);
  fs.writeFileSync(sumsPath, `${kept.join("\n")}\n`);

  console.log("");
  console.log(`Platform package : ${dir}`);
  console.log(`  name           : ${platformPackage(key)}`);
  console.log(`  version        : ${version}`);
  console.log(`  binary         : ${destBin} (${(fs.statSync(destBin).size / 1048576).toFixed(2)} MB)`);
  console.log(`  sha256         : ${hash}`);
  console.log(`Release asset    : ${path.join(DIST_DIR, asset)}`);
  console.log("");
  // leading ./ 不可省：npm 会把 owner/repo 形式误认为 GitHub shorthand
  console.log(`Publish with: npm publish ./${path.relative(ROOT, dir).split(path.sep).join("/")}`);
}

main();