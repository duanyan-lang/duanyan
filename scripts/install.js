#!/usr/bin/env node
"use strict";

const { execSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");

const root = path.join(__dirname, "..");
const ext = os.platform() === "win32" ? ".exe" : "";
const targetBin = path.join(root, "target", "release", `hardy${ext}`);

// Skip if binary already exists
if (fs.existsSync(targetBin)) {
  console.log("\x1b[32m✓ hardy binary already exists, skipping build.\x1b[0m");
  process.exit(0);
}

// Check if cargo is available
try {
  execSync("cargo --version", { stdio: "pipe" });
} catch {
  console.log("\x1b[33m⚠ Rust/Cargo not found. Skipping native build.\x1b[0m");
  console.log("  Install Rust: https://rustup.rs");
  console.log("  Then run: npm run build");
  process.exit(0); // Don't fail install
}

// Build from source
console.log("\x1b[36m⚙ Building HardyScript compiler from source...\x1b[0m");
try {
  execSync("cargo build --release", {
    cwd: root,
    stdio: "inherit",
    timeout: 300000, // 5 min timeout
  });
  console.log("\x1b[32m✓ HardyScript built successfully!\x1b[0m");
  console.log(`  Binary: ${targetBin}`);
} catch (err) {
  console.log("\x1b[33m⚠ Build failed. You can build manually later:\x1b[0m");
  console.log("  cd " + root + " && cargo build --release");
  process.exit(0); // Don't fail npm install
}
