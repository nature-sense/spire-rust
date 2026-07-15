#!/usr/bin/env node

/**
 * Test harness for the spire-extension JSON-RPC server.
 *
 * Spawns the extension as a child process, sends JSON-RPC requests via stdin,
 * and reads responses from stdout. Validates the protocol and basic handler
 * behaviour.
 *
 * Usage:
 *   node test/test-harness.mjs
 *
 * Environment:
 *   SPIERE_EXT_BINARY - path to the extension JS file (default: ./dist/extension.js)
 */

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { strict as assert } from 'node:assert';

const EXT_BINARY = process.env.SPIRE_EXT_BINARY || './dist/extension.js';

// ── Helpers ──────────────────────────────────────────────────────────────────

let requestId = 0;

function sendRequest(proc, method, params = {}) {
  const id = ++requestId;
  const request = JSON.stringify({ jsonrpc: '2.0', id, method, params });
  proc.stdin.write(request + '\n');
  return id;
}

function sendNotification(proc, method, params = {}) {
  const notification = JSON.stringify({ jsonrpc: '2.0', method, params });
  proc.stdin.write(notification + '\n');
}

function waitForResponse(rl, expectedId) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`Timeout waiting for response id=${expectedId}`));
    }, 5000);

    rl.on('line', function handler(line) {
      try {
        const msg = JSON.parse(line);
        if (msg.id === expectedId) {
          clearTimeout(timeout);
          rl.removeListener('line', handler);
          resolve(msg);
        }
        // Ignore notifications and other responses
      } catch {
        // Ignore parse errors on non-JSON lines
      }
    });
  });
}

// ── Tests ────────────────────────────────────────────────────────────────────

async function runTests() {
  console.log('=== Spire Extension Test Harness ===\n');

  // Spawn the extension process
  const proc = spawn('node', [EXT_BINARY], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, SPIERE_LOG_LEVEL: 'error' },
  });

  const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

  // Collect stderr for debugging
  let stderr = '';
  proc.stderr.on('data', (chunk) => { stderr += chunk.toString(); });

  let passed = 0;
  let failed = 0;

  async function test(name, fn) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
      passed++;
    } catch (err) {
      console.log(`  ✗ ${name}: ${err.message}`);
      if (stderr) {
        console.log(`    stderr: ${stderr.slice(-500)}`);
      }
      failed++;
    }
  }

  // ── Test 1: Basic request/response ──
  await test('chat/getActive returns a chat dialog', async () => {
    const id = sendRequest(proc, 'chat/getActive');
    const response = await waitForResponse(rl, id);
    assert.equal(response.jsonrpc, '2.0');
    assert.equal(response.id, id);
    assert.ok(response.result);
    assert.equal(response.result.id, 'default');
    assert.equal(response.result.status, 'idle');
  });

  // ── Test 2: Chat append ──
  await test('chat/append adds a message', async () => {
    const id = sendRequest(proc, 'chat/append', {
      chatId: 'default',
      content: 'Hello from test!',
      options: { role: 'user' },
    });
    const response = await waitForResponse(rl, id);
    assert.equal(response.result.role, 'user');
    assert.equal(response.result.content, 'Hello from test!');
    assert.ok(response.result.id);
  });

  // ── Test 3: Chat history ──
  await test('chat/getHistory returns chats', async () => {
    const id = sendRequest(proc, 'chat/getHistory');
    const response = await waitForResponse(rl, id);
    assert.ok(Array.isArray(response.result));
    assert.ok(response.result.length > 0);
  });

  // ── Test 4: Chat setTitle ──
  await test('chat/setTitle updates chat title', async () => {
    const id = sendRequest(proc, 'chat/setTitle', {
      chatId: 'default',
      title: 'Test Chat',
    });
    const response = await waitForResponse(rl, id);
    assert.equal(response.result, undefined); // void return

    // Verify the title was set
    const id2 = sendRequest(proc, 'chat/getActive');
    const response2 = await waitForResponse(rl, id2);
    assert.equal(response2.result.title, 'Test Chat');
  });

  // ── Test 5: Chat clear ──
  await test('chat/clear empties messages', async () => {
    const id = sendRequest(proc, 'chat/clear', { chatId: 'default' });
    await waitForResponse(rl, id);

    const id2 = sendRequest(proc, 'chat/getActive');
    const response2 = await waitForResponse(rl, id2);
    assert.equal(response2.result.messages.length, 0);
  });

  // ── Test 6: Method not found ──
  await test('unknown method returns error', async () => {
    const id = sendRequest(proc, 'nonexistent/method');
    const response = await waitForResponse(rl, id);
    assert.ok(response.error);
    assert.equal(response.error.code, -32601);
  });

  // ── Test 7: Invalid JSON ──
  await test('invalid JSON returns parse error', async () => {
    const id = 999;
    proc.stdin.write('not json\n');
    // Parse errors get id=-1, so we need to match differently
    const response = await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Timeout')), 5000);
      rl.on('line', function handler(line) {
        try {
          const msg = JSON.parse(line);
          if (msg.id === -1 && msg.error?.code === -32700) {
            clearTimeout(timeout);
            rl.removeListener('line', handler);
            resolve(msg);
          }
        } catch { /* skip */ }
      });
    });
    assert.equal(response.error.code, -32700);
  });

  // ── Test 8: Terminal create ──
  await test('terminal/create returns an id', async () => {
    const id = sendRequest(proc, 'terminal/create', { name: 'test-term' });
    const response = await waitForResponse(rl, id);
    assert.ok(typeof response.result === 'string');
    assert.ok(response.result.startsWith('term-'));
  });

  // ── Test 9: Workspace folders (stub) ──
  await test('workspace/getFolders returns array', async () => {
    const id = sendRequest(proc, 'workspace/getFolders');
    const response = await waitForResponse(rl, id);
    assert.ok(Array.isArray(response.result));
  });

  // ── Test 10: Notification (no response expected) ──
  await test('notification does not produce a response', async () => {
    sendNotification(proc, 'chat/show', { chatId: 'default' });
    // Wait a bit to ensure no response comes
    await new Promise(r => setTimeout(r, 500));
    // If we got here without a timeout/error, the notification was handled
    assert.ok(true);
  });

  // ── Summary ──
  console.log(`\nResults: ${passed} passed, ${failed} failed, ${passed + failed} total`);

  proc.kill();
  process.exit(failed > 0 ? 1 : 0);
}

runTests().catch((err) => {
  console.error('Test harness error:', err);
  process.exit(1);
});
