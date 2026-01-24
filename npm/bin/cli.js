#!/usr/bin/env node

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const PLATFORMS = {
  "darwin-arm64": "@sh3ll3x3c/native-devtools-mcp-darwin-arm64",
  "win32-x64": "@sh3ll3x3c/native-devtools-mcp-win32-x64",
};

function getPlatformPackage() {
  const platform = process.platform;
  const arch = process.arch;
  const key = `${platform}-${arch}`;

  const pkg = PLATFORMS[key];
  if (!pkg) {
    console.error(`Unsupported platform: ${platform}-${arch}`);
    console.error(
      "native-devtools-mcp supports: darwin-arm64 (Apple Silicon), win32-x64 (Windows x64)"
    );
    process.exit(1);
  }

  return pkg;
}

function findBinary() {
  const platform = process.platform;
  const arch = process.arch;
  const platformDir = `${platform}-${arch}`;
  const pkg = getPlatformPackage();

  // Binary name differs by platform
  const binaryName =
    platform === "win32" ? "native-devtools-mcp.exe" : "native-devtools-mcp";

  // Try to find the platform-specific package
  const possiblePaths = [
    // Local development (binary in sibling directory)
    path.join(__dirname, "..", platformDir, "bin", binaryName),
    // When installed as a dependency
    path.join(__dirname, "..", "node_modules", pkg, "bin", binaryName),
    // When installed globally or via npx
    path.join(__dirname, "..", "..", pkg, "bin", binaryName),
    // Hoisted in node_modules
    path.join(__dirname, "..", "..", "..", pkg, "bin", binaryName),
  ];

  for (const binPath of possiblePaths) {
    if (fs.existsSync(binPath)) {
      return binPath;
    }
  }

  console.error(`Could not find binary for ${pkg}`);
  console.error("Searched paths:");
  possiblePaths.forEach((p) => console.error(`  - ${p}`));
  console.error("\nTry reinstalling: npm install -g native-devtools-mcp");
  process.exit(1);
}

const binaryPath = findBinary();

const child = spawn(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
});

child.on("error", (err) => {
  console.error(`Failed to start native-devtools-mcp: ${err.message}`);
  process.exit(1);
});

child.on("close", (code) => {
  process.exit(code ?? 0);
});
