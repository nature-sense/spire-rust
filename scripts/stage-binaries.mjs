#!/usr/bin/env node

/**
 * Stage compiled Rust binaries and embedding model into the VS Code
 * extension's bin/ directory.
 *
 * After `cargo build --release --workspace`, this script copies the
 * platform-specific binaries and the embedding model files into
 * ts/spire-extension/bin/<platform>/ so they are bundled into the .vsix
 * package.
 *
 * Usage:
 *   node scripts/stage-binaries.mjs
 */

import { execSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'fs';
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
  { name: 'spire-core', crate: 'rust/spire-core' },
  { name: 'mcp-git', crate: 'rust/mcp/mcp-git' },
  { name: 'mcp-process', crate: 'rust/mcp/mcp-process' },
  { name: 'mcp-search', crate: 'rust/mcp/mcp-search' },
  { name: 'mcp-terminal', crate: 'rust/mcp/mcp-terminal' },
  { name: 'mcp-filesystem', crate: 'rust/mcp/mcp-filesystem' },
  { name: 'mcp-cargo', crate: 'rust/mcp/mcp-cargo' },
];


const ext = process.platform === 'win32' ? '.exe' : '';

// ── Copy binaries ────────────────────────────────────────────────────────

const targetDir = join(root, 'target', 'release');
const destDir = join(root, 'ts', 'spire-extension', 'bin', platformDir);

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

// ── Stage embedding model ────────────────────────────────────────────────

/**
 * Copy the embedding model files from the HuggingFace cache into the
 * extension's bin/<platform>/models/all-MiniLM-L6-v2/ directory so they
 * are bundled in the VSIX and available on first launch without network.
 */
const MODEL_ID = 'sentence-transformers/all-MiniLM-L6-v2';
const MODEL_DIR_NAME = 'all-MiniLM-L6-v2';
const MODEL_FILES = ['config.json', 'tokenizer.json', 'model.safetensors'];

// Resolve the HF cache path
const hfHome = process.env.HF_HOME || join(process.env.HOME || '~', '.cache', 'huggingface');
const hfCacheDir = join(hfHome, 'hub');

// The HF cache stores models as: models--sentence-transformers--all-MiniLM-L6-v2
const cacheModelDir = join(hfCacheDir, `models--${MODEL_ID.replace(/\//g, '--')}`);

// Find the snapshot directory (there's usually one under snapshots/<hash>/)
let snapshotDir = null;
if (existsSync(cacheModelDir)) {
  const snapshotsPath = join(cacheModelDir, 'snapshots');
  if (existsSync(snapshotsPath)) {
    const entries = readdirSync(snapshotsPath);
    for (const entry of entries) {
      const fullPath = join(snapshotsPath, entry);
      if (statSync(fullPath).isDirectory()) {
        snapshotDir = fullPath;
        break;
      }
    }
  }
}

if (snapshotDir) {
  const modelDestDir = join(destDir, 'models', MODEL_DIR_NAME);
  if (!existsSync(modelDestDir)) {
    mkdirSync(modelDestDir, { recursive: true });
  }

  for (const file of MODEL_FILES) {
    const src = join(snapshotDir, file);
    const dest = join(modelDestDir, file);

    if (!existsSync(src)) {
      console.warn(`⚠  Model file not found: ${src} — skipping ${file}`);
      continue;
    }

    copyFileSync(src, dest);
    const sizeMB = (statSync(src).size / (1024 * 1024)).toFixed(1);
    console.log(`✓  models/${MODEL_DIR_NAME}/${file} (${sizeMB} MB) → ${dest}`);
  }

  console.log(`\nEmbedding model staged to: ${modelDestDir}`);
} else {
  console.warn(`\n⚠  Embedding model not found in HF cache at ${cacheModelDir}`);
  console.warn('   Run "scripts/download-embedding-model.sh" first to download it.');
  console.warn('   The extension will fall back to downloading at runtime.');
}

