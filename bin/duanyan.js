#!/usr/bin/env node
"use strict";

const { execFileSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");

// Locate the duanyan binary
function findBinary() {
  const ext = os.platform() === "win32" ? ".exe" : "";
  const binName = `duanyan${ext}`;

  // 1. Check package-local binary (installed by postinstall)
  const localBin = path.join(__dirname, "..", "target", "release", binName);
  if (fs.existsSync(localBin)) return localBin;

  // 2. Check cargo bin directory
  const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
  const cargoBin = path.join(cargoHome, "bin", binName);
  if (fs.existsSync(cargoBin)) return cargoBin;

  // 3. Hope it's in PATH
  return binName;
}

const binary = findBinary();
const args = process.argv.slice(2);

try {
  const result = execFileSync(binary, args, {
    stdio: "inherit",
    env: { ...process.env },
  });
} catch (err) {
  if (err.status !== null) {
    process.exit(err.status);
  }
  console.error(`\x1b[31mError: Cannot find 'duanyan' binary.\x1b[0m`);
  console.error(`Please install Rust (https://rustup.rs) and run:`);
  console.error(`  cd ${path.join(__dirname, "..")} && cargo install --path . --force`);
  process.exit(1);
}
