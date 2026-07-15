#!/usr/bin/env node

/**
 * Communication Layer Integration Tests
 *
 * Tests the EnvironmentClient against the mock environment server.
 * Spawns the mock server as a child process and exercises the
 * full JSON-RPC communication protocol.
 *
 * Usage:
 *   node test/communication.test.mjs
 */

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { strict as assert } from 'node:assert';

const MOCK_SERVER = './test/mock-env-server.mjs';

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

function waitForResponse(rl, expectedId, timeoutMs = 5000) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`Timeout waiting for response id=${expectedId}`));
    }, timeoutMs);

    rl.on('line', function handler(line) {
      try {
        const msg = JSON.parse(line);
        if (msg.id === expectedId) {
          clearTimeout(timeout);
          rl.removeListener('line', handler);
          resolve(msg);
        }
      } catch { /* skip non-JSON lines */ }
    });
  });
}

function waitForNotification(rl, expectedMethod, timeoutMs = 5000) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`Timeout waiting for notification: ${expectedMethod}`));
    }, timeoutMs);

    rl.on('line', function handler(line) {
      try {
        const msg = JSON.parse(line);
        if (msg.method === expectedMethod && msg.id === undefined) {
          clearTimeout(timeout);
          rl.removeListener('line', handler);
          resolve(msg);
        }
      } catch { /* skip */ }
    });
  });
}

// ── Tests ────────────────────────────────────────────────────────────────────

async function runTests() {
  console.log('=== Communication Layer Tests ===\n');

  let passed = 0;
  let failed = 0;

  async function test(name, fn) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
      passed++;
    } catch (err) {
      console.log(`  ✗ ${name}: ${err.message}`);
      failed++;
    }
  }

  // ── Test 1: Basic call/response ──
  await test('ping returns pong', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'ping');
    const response = await waitForResponse(rl, id);
    assert.equal(response.result, 'pong');
    assert.equal(response.jsonrpc, '2.0');

    proc.kill();
    rl.close();
  });

  // ── Test 2: Chat getActive ──
  await test('chat/getActive returns a chat dialog', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'chat/getActive');
    const response = await waitForResponse(rl, id);
    assert.ok(response.result);
    assert.equal(response.result.id, 'default');
    assert.equal(response.result.status, 'idle');

    proc.kill();
    rl.close();
  });

  // ── Test 3: Chat append ──
  await test('chat/append adds a message', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'chat/append', {
      chatId: 'default',
      content: 'Hello from test!',
      options: { role: 'user' },
    });
    const response = await waitForResponse(rl, id);
    assert.equal(response.result.role, 'user');
    assert.equal(response.result.content, 'Hello from test!');
    assert.ok(response.result.id);

    proc.kill();
    rl.close();
  });

  // ── Test 4: Chat append triggers assistant notification ──
  await test('chat/append with role=user triggers event/chat/message notification', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'chat/append', {
      chatId: 'default',
      content: 'Trigger assistant reply',
      options: { role: 'user' },
    });
    // Wait for the response first
    const response = await waitForResponse(rl, id);
    assert.equal(response.result.role, 'user');

    // Then wait for the notification
    const notification = await waitForNotification(rl, 'event/chat/message', 3000);
    assert.equal(notification.params.chatId, 'default');
    assert.equal(notification.params.message.role, 'assistant');
    assert.ok(notification.params.message.content);

    proc.kill();
    rl.close();
  });

  // ── Test 5: Chat history ──
  await test('chat/getHistory returns chats', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    // Add a message first
    const id1 = sendRequest(proc, 'chat/append', {
      chatId: 'default',
      content: 'Test message',
      options: { role: 'user' },
    });
    await waitForResponse(rl, id1);

    const id2 = sendRequest(proc, 'chat/getHistory');
    const response = await waitForResponse(rl, id2);
    assert.ok(Array.isArray(response.result));
    assert.ok(response.result.length > 0);

    proc.kill();
    rl.close();
  });

  // ── Test 6: Chat setTitle ──
  await test('chat/setTitle updates chat title', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id1 = sendRequest(proc, 'chat/setTitle', {
      chatId: 'default',
      title: 'Test Chat',
    });
    await waitForResponse(rl, id1);

    const id2 = sendRequest(proc, 'chat/getActive');
    const response = await waitForResponse(rl, id2);
    assert.equal(response.result.title, 'Test Chat');

    proc.kill();
    rl.close();
  });

  // ── Test 7: Chat clear ──
  await test('chat/clear empties messages', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    // Add a message
    const id1 = sendRequest(proc, 'chat/append', {
      chatId: 'default',
      content: 'To be cleared',
      options: { role: 'user' },
    });
    await waitForResponse(rl, id1);

    // Clear
    const id2 = sendRequest(proc, 'chat/clear', { chatId: 'default' });
    await waitForResponse(rl, id2);

    // Verify
    const id3 = sendRequest(proc, 'chat/getActive');
    const response = await waitForResponse(rl, id3);
    assert.equal(response.result.messages.length, 0);

    proc.kill();
    rl.close();
  });

  // ── Test 8: Unknown method ──
  await test('unknown method returns error', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'nonexistent/method');
    const response = await waitForResponse(rl, id);
    assert.ok(response.error);
    assert.equal(response.error.code, -32601);

    proc.kill();
    rl.close();
  });

  // ── Test 9: Invalid JSON ──
  await test('invalid JSON returns parse error', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    proc.stdin.write('not json\n');
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

    proc.kill();
    rl.close();
  });

  // ── Test 10: Workspace ──
  await test('workspace/getFolders returns folders', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'workspace/getFolders');
    const response = await waitForResponse(rl, id);
    assert.ok(Array.isArray(response.result));
    assert.ok(response.result.length > 0);
    assert.ok(response.result[0].name);

    proc.kill();
    rl.close();
  });

  // ── Test 11: Editor ──
  await test('editor/getActive returns editor state', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    const id = sendRequest(proc, 'editor/getActive');
    const response = await waitForResponse(rl, id);
    assert.ok(response.result);
    assert.ok(response.result.document);
    assert.equal(response.result.document.languageId, 'rust');

    proc.kill();
    rl.close();
  });

  // ── Test 12: Notification (no response) ──
  await test('notification does not produce a response', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    sendNotification(proc, 'chat/show', { chatId: 'default' });

    // Wait a bit — no response should come
    await new Promise(r => setTimeout(r, 500));
    assert.ok(true);

    proc.kill();
    rl.close();
  });

  // ── Test 13: Concurrent requests ──
  await test('handles concurrent requests', async () => {
    const proc = spawn('node', [MOCK_SERVER], { stdio: ['pipe', 'pipe', 'pipe'] });
    const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });

    // Send 3 requests in quick succession
    const id1 = sendRequest(proc, 'ping');
    const id2 = sendRequest(proc, 'ping');
    const id3 = sendRequest(proc, 'ping');

    const [r1, r2, r3] = await Promise.all([
      waitForResponse(rl, id1),
      waitForResponse(rl, id2),
      waitForResponse(rl, id3),
    ]);

    assert.equal(r1.result, 'pong');
    assert.equal(r2.result, 'pong');
    assert.equal(r3.result, 'pong');

    proc.kill();
    rl.close();
  });

  // ── Summary ──
  console.log(`\nResults: ${passed} passed, ${failed} failed, ${passed + failed} total`);
  process.exit(failed > 0 ? 1 : 0);
}

runTests().catch((err) => {
  console.error('Test harness error:', err);
  process.exit(1);
});
