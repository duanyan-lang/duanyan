# @duanyan-lang/duanyan 平台子包

这些目录是 **内部平台子包**，每个只装一个平台的预编译 `duanyan` 二进制。

**请不要直接安装它们。** 安装主包即可，npm 会依据子包的 `os` / `cpu` 字段
自动只下载当前平台那一个：

```bash
npm install -g @duanyan-lang/duanyan
```

## 平台矩阵

| 子包 | 平台 | Rust target |
|---|---|---|
| `@duanyan-lang/duanyan-win32-x64` | Windows x64 | `x86_64-pc-windows-msvc` |
| `@duanyan-lang/duanyan-win32-arm64` | Windows ARM64 | `aarch64-pc-windows-msvc` |
| `@duanyan-lang/duanyan-linux-x64` | Linux x64 | `x86_64-unknown-linux-gnu` |
| `@duanyan-lang/duanyan-linux-arm64` | Linux ARM64 | `aarch64-unknown-linux-gnu` |
| `@duanyan-lang/duanyan-darwin-x64` | macOS Intel | `x86_64-apple-darwin` |
| `@duanyan-lang/duanyan-darwin-arm64` | macOS Apple Silicon | `aarch64-apple-darwin` |

矩阵的单一真值是 [`scripts/platforms.js`](../scripts/platforms.js)；
调整平台时改那里，再同步 `.github/workflows/release.yml`。

## 目录内容

- `package.json` / `index.js` — 纳入版本库的模板。`index.js` 导出二进制绝对路径，
  主包的 `bin/duanyan.js` 用 `require("<子包名>")` 直接取得。
- `bin/`、`README.md`、`LICENSE` — 由 `scripts/build-platform-package.js` 生成的产物，
  已被 `.gitignore` 排除，不入库。

## 本地组装

```bash
node scripts/build-platform-package.js              # 当前平台
node scripts/build-platform-package.js --no-build   # 复用已有二进制
```

产出 `npm/<key>/`（可 `npm publish`）与 `dist/<asset>` + `dist/SHA256SUMS.txt`（Release 资产）。

## 发布顺序（铁律）

**6 个平台子包全部发布成功后，才发布主包。** 顺序反了会让主包的
`optionalDependencies` 解析 404，用户被迫退化到下载兜底。
CI 的 `publish-npm.yml` 已按此顺序实现，任一子包失败即中止。

## 许可

MIT，与主包一致（见各子包内由根 `LICENSE` 复制而来的副本）。