#!/usr/bin/env node

/**
 * Stage compiled Rust binaries into the VS Code extension's bin/ directory.
 *
 * After `cargo build --release --workspace`, this script copies the
 * platform-specific binaries into spire-extension/bin/<platform>/ so they
 * are bundled into the .vsix package.
 *
 * Usage:
 *   node scripts/stage-binaries.mjs
 */

import { execSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');

// ── Detect platform ──────────────────────────────────────────────────────

const platformMap = {
  darwin: 'darwin',
  linux: 'linux',
  win32: 'win32',
};

const archMap = {
  x64: 'x64',
  arm64: 'arm64',
  arm: 'arm',
};

const os = platformMap[process.platform];
const arch = archMap[process.arch];

if (!os || !arch) {
  console.error(`Unsupported platform: ${process.platform} ${process.arch}`);
  process.exit(1);
}

const platformDir = `${os}-${arch}`;

// ── Binary names ─────────────────────────────────────────────────────────

const binaries = [
  { name: 'spire-core', crate: 'spire-core' },
  { name: 'mcp-git', crate: 'mcp/mcp-git' },
  { name: 'mcp-process', crate: 'mcp/mcp-process' },
  { name: 'mcp-search', crate: 'mcp/mcp-search' },
];

const ext = process.platform === 'win32' ? '.exe' : '';

// ── Copy binaries ────────────────────────────────────────────────────────

const targetDir = join(root, 'target', 'release');
const destDir = join(root, 'spire-extension', 'bin', platformDir);

if (!existsSync(destDir)) {
  mkdirSync(destDir, { recursive: true });
}

for (const bin of binaries) {
  const src = join(targetDir, `${bin.name}${ext}`);
  const dest = join(destDir, `${bin.name}${ext}`);

  if (!existsSync(src)) {
    console.warn(`⚠  Binary not found: ${src} — skipping ${bin.name}`);
    continue;
  }

  copyFileSync(src, dest);
  console.log(`✓  ${bin.name} → ${dest}`);

  // Make executable on Unix
  if (process.platform !== 'win32') {
    try {
      execSync(`chmod +x "${dest}"`);
    } catch {
      // non-critical
    }
  }
}

console.log(`\nBinaries staged to: ${destDir}`);
