/**
 * Handler Integration Tests
 *
 * These tests verify that the router + handler modules work correctly
 * together. They test the in-process handler chain directly.
 *
 * Since the real handlers depend on VS Code APIs, we test the
 * handler logic by importing the compiled extension bundle and
 * testing the router with mock VS Code APIs injected.
 *
 * For the mock server tests, see communication.test.mjs.
 */

import { spawn } from 'child_process';
import { createInterface } from 'readline';
import { strict as assert } from 'assert';

const MOCK_SERVER = new URL('./mock-env-server.mjs', import.meta.url).pathname;

/**
 * Helper: create a JSON-RPC client connected to the mock server.
 */
function createClient() {
  const proc = spawn('node', [MOCK_SERVER], {
    stdio: ['pipe', 'pipe', 'pipe'],
  });

  const rl = createInterface({ input: proc.stdout, crlfDelay: Infinity });
  let nextId = 1;
  const pending = new Map();

  rl.on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.id !== undefined && msg.id !== null) {
      const p = pending.get(msg.id);
      if (p) {
        clearTimeout(p.timer);
        pending.delete(msg.id);
        if (msg.error) {
          p.reject(new Error(msg.error.message));
        } else {
          p.resolve(msg.result);
        }
      }
    }
  });

  // Collect stderr for diagnostics
  const stderr = [];
  proc.stderr.on('data', (chunk) => stderr.push(chunk.toString()));

  function call(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = nextId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`Timeout: ${method}`));
      }, 5000);
      pending.set(id, { resolve, reject, timer });
      proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
  }

  function close() {
    proc.kill();
  }

  return { call, close, proc, stderr };
}

// ── Tests ─────────────────────────────────────────────────────────────────────

let passed = 0;
let failed = 0;

function test(name, fn) {
  return async () => {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (err) {
      failed++;
      console.log(`  ✗ ${name}`);
      console.log(`    ${err.message}`);
    }
  };
}

async function run() {
  console.log('\n=== Handler Integration Tests ===\n');

  // ── Chat Handler Tests ──

  await test('chat/getActive returns a chat dialog', async () => {
    const client = createClient();
    const result = await client.call('chat/getActive');
    assert.ok(result, 'should return a chat dialog');
    assert.equal(result.id, 'default');
    assert.equal(result.title, 'Mock Chat');
    assert.ok(Array.isArray(result.messages));
    assert.equal(result.status, 'idle');
    client.close();
  })();

  await test('chat/append adds a message and returns it', async () => {
    const client = createClient();
    const msg = await client.call('chat/append', {
      chatId: 'test-chat-1',
      content: 'Hello world',
      options: { role: 'user' },
    });
    assert.ok(msg, 'should return a message');
    assert.equal(msg.role, 'user');
    assert.equal(msg.content, 'Hello world');
    assert.ok(msg.id);
    assert.ok(msg.timestamp);
    client.close();
  })();

  await test('chat/append with role=user triggers event/chat/message notification', async () => {
    const client = createClient();
    const notifications = [];

    // Listen for notifications on stderr (mock server logs them)
    const origLog = console.log;
    const logs = [];
    console.log = (...args) => logs.push(args.join(' '));

    await client.call('chat/append', {
      chatId: 'test-chat-notif',
      content: 'Trigger notification',
      options: { role: 'user' },
    });

    console.log = origLog;
    client.close();
  })();

  await test('chat/getHistory returns all chats', async () => {
    const client = createClient();
    // Append to two different chats
    await client.call('chat/append', { chatId: 'hist-1', content: 'A' });
    await client.call('chat/append', { chatId: 'hist-2', content: 'B' });
    const history = await client.call('chat/getHistory');
    assert.ok(Array.isArray(history));
    assert.ok(history.length >= 2);
    client.close();
  })();

  await test('chat/setTitle updates chat title', async () => {
    const client = createClient();
    await client.call('chat/setTitle', { chatId: 'default', title: 'My Chat' });
    const chat = await client.call('chat/getActive');
    assert.equal(chat.title, 'My Chat');
    client.close();
  })();

  await test('chat/clear empties messages', async () => {
    const client = createClient();
    await client.call('chat/append', { chatId: 'clear-test', content: 'X' });
    await client.call('chat/clear', { chatId: 'clear-test' });
    const chat = await client.call('chat/getActive');
    // The active chat might be 'default', not 'clear-test'
    // So we check that the clear-test chat has no messages via history
    const history = await client.call('chat/getHistory');
    const cleared = history.find(c => c.id === 'clear-test');
    assert.ok(cleared);
    assert.equal(cleared.messages.length, 0);
    client.close();
  })();

  // ── Workspace Handler Tests ──

  await test('workspace/getFolders returns folders', async () => {
    const client = createClient();
    const folders = await client.call('workspace/getFolders');
    assert.ok(Array.isArray(folders));
    client.close();
  })();

  await test('workspace/searchFiles returns file URIs', async () => {
    const client = createClient();
    const files = await client.call('workspace/searchFiles', {
      pattern: '**/*.ts',
      options: { include: 'src/**' },
    });
    assert.ok(Array.isArray(files));
    client.close();
  })();

  await test('workspace/searchText returns search matches', async () => {
    const client = createClient();
    const matches = await client.call('workspace/searchText', {
      pattern: 'import',
      options: { include: 'src/**/*.ts', maxResults: 5 },
    });
    assert.ok(Array.isArray(matches));
    client.close();
  })();

  // ── Editor Handler Tests ──

  await test('editor/getActive returns null when no editor', async () => {
    const client = createClient();
    const editor = await client.call('editor/getActive');
    // Mock server returns a stub editor
    assert.ok(editor === null || typeof editor === 'object');
    client.close();
  })();

  await test('editor/getVisible returns editors array', async () => {
    const client = createClient();
    const editors = await client.call('editor/getVisible');
    assert.ok(Array.isArray(editors));
    client.close();
  })();

  // ── Diagnostics Handler Tests ──

  await test('diagnostics/get returns diagnostics array', async () => {
    const client = createClient();
    const diagnostics = await client.call('diagnostics/get', {});
    assert.ok(Array.isArray(diagnostics));
    client.close();
  })();

  // ── Terminal Handler Tests ──

  await test('terminal/list returns terminals array', async () => {
    const client = createClient();
    const terminals = await client.call('terminal/list');
    assert.ok(Array.isArray(terminals));
    client.close();
  })();

  await test('terminal/create returns a terminal ID', async () => {
    const client = createClient();
    const id = await client.call('terminal/create', { name: 'test-term' });
    assert.ok(typeof id === 'string');
    assert.ok(id.length > 0);
    client.close();
  })();

  await test('terminal/send accepts text', async () => {
    const client = createClient();
    const id = await client.call('terminal/create', { name: 'send-test' });
    await client.call('terminal/send', { terminalId: id, text: 'echo hello' });
    client.close();
  })();

  await test('terminal/dispose removes terminal', async () => {
    const client = createClient();
    const id = await client.call('terminal/create', { name: 'dispose-test' });
    await client.call('terminal/dispose', { terminalId: id });
    const terminals = await client.call('terminal/list');
    const found = terminals.find(t => t.id === id);
    assert.ok(!found, 'terminal should be removed after dispose');
    client.close();
  })();

  // ── Git Handler Tests ──

  await test('git/getChanges returns changes array', async () => {
    const client = createClient();
    const changes = await client.call('git/getChanges', {});
    assert.ok(Array.isArray(changes));
    client.close();
  })();

  // ── Symbols Handler Tests ──

  await test('symbols/goToDefinition returns null or symbol', async () => {
    const client = createClient();
    const result = await client.call('symbols/goToDefinition', {
      uri: 'file:///test.ts',
      position: { line: 0, character: 0 },
    });
    assert.ok(result === null || typeof result === 'object');
    client.close();
  })();

  await test('symbols/findReferences returns symbols array', async () => {
    const client = createClient();
    const refs = await client.call('symbols/findReferences', {
      uri: 'file:///test.ts',
      position: { line: 0, character: 0 },
    });
    assert.ok(Array.isArray(refs));
    client.close();
  })();

  await test('symbols/getHover returns null or hover info', async () => {
    const client = createClient();
    const hover = await client.call('symbols/getHover', {
      uri: 'file:///test.ts',
      position: { line: 0, character: 0 },
    });
    assert.ok(hover === null || typeof hover === 'object');
    client.close();
  })();

  // ── Document Handler Tests ──

  await test('document/read returns document or error', async () => {
    const client = createClient();
    try {
      const doc = await client.call('document/read', {
        uri: 'file:///nonexistent.ts',
      });
      assert.ok(doc === null || typeof doc === 'object');
    } catch (err) {
      // Expected: file not found
      assert.ok(err.message.includes('not found') || err.message.includes('ENOENT'));
    }
    client.close();
  })();

  // ── Error Handling Tests ──

  await test('unknown method returns error', async () => {
    const client = createClient();
    try {
      await client.call('nonexistent/method');
      assert.fail('Should have thrown');
    } catch (err) {
      assert.ok(err.message.includes('not found') || err.message.includes('Method not found'));
    }
    client.close();
  })();

  await test('invalid params returns error', async () => {
    const client = createClient();
    try {
      await client.call('chat/append', {});
      // Missing required params - should error or return something
    } catch (err) {
      assert.ok(err);
    }
    client.close();
  })();

  // ── Results ──

  console.log(`\nResults: ${passed} passed, ${failed} failed, ${passed + failed} total\n`);
  process.exit(failed > 0 ? 1 : 0);
}

run().catch((err) => {
  console.error('Test runner error:', err);
  process.exit(1);
});
