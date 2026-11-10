# 把最新 release 同步到 PATH 上的 bin\duanyan.exe。
# 本机 `duanyan` 命令解析到 C:\OPENSNAR\DUANYAN\bin\duanyan.exe（PATH 直连，
# ~/.cargo/bin 里没有 duanyan），删掉它 = 命令消失；不同步它 = 停留在旧版。
# 用法：每次 cargo build --release 之后跑一次本脚本。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root "target\release\duanyan.exe"
$dst = Join-Path $root "bin\duanyan.exe"

if (-not (Test-Path $src)) {
    Write-Error "未找到 $src，请先 cargo build --release"
}
Copy-Item $src $dst -Force
Write-Host "已同步: $src -> $dst"
& $dst version | Select-String 'v\d+\.\d+\.\d+'
