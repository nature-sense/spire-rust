// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

import { ChildProcess, spawn } from "child_process";
import * as path from "path";
import { EventEmitter } from "events";

/**
 * Manages the connection to the Rust MCP server via stdio.
 * Handles JSON-RPC over stdio communication.
 */
export class McpClient extends EventEmitter {
  private process: ChildProcess | null = null;
  private buffer: string = "";
  private pendingRequests: Map<
    string,
    { resolve: (value: any) => void; reject: (reason: any) => void }
  > = new Map();
  private requestId: number = 0;
  private binaryPath: string;

  constructor(binaryPath?: string) {
    super();
    this.binaryPath =
      binaryPath ||
      path.resolve(__dirname, "..", "bin", "spire-rust");
  }

  /**
   * Connects to the Rust MCP server by spawning it as a subprocess.
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        this.process = spawn(this.binaryPath, [], {
          stdio: ["pipe", "pipe", "pipe"],
        });

        this.process.stdout?.on("data", (data: Buffer) => {
          this.buffer += data.toString();
          this.processBuffer();
        });

        this.process.stderr?.on("data", (data: Buffer) => {
          console.error(`[spire-rust stderr] ${data.toString().trim()}`);
        });

        this.process.on("error", (err: Error) => {
          console.error("Failed to start spire-rust process:", err);
          this.emit("error", err);
          reject(err);
        });

        this.process.on("exit", (code: number | null) => {
          console.log(`spire-rust process exited with code ${code}`);
          this.emit("exit", code);
        });

        // Resolve once the process is spawned
        resolve();
      } catch (err) {
        reject(err);
      }
    });
  }

  /**
   * Processes the incoming data buffer, extracting complete JSON-RPC messages.
   */
  private processBuffer(): void {
    const lines = this.buffer.split("\n");
    // Keep the last incomplete line in the buffer
    this.buffer = lines.pop() || "";

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) continue;

      try {
        const message = JSON.parse(trimmed);

        if (message.id !== undefined && message.id !== null) {
          // This is a response to a request
          const pending = this.pendingRequests.get(String(message.id));
          if (pending) {
            this.pendingRequests.delete(String(message.id));
            if (message.error) {
              pending.reject(new Error(message.error.message));
            } else {
              pending.resolve(message.result);
            }
          }
        } else if (message.method) {
          // This is a notification from the server
          this.emit("notification", message);
        }
      } catch (err) {
        console.error("Failed to parse MCP message:", trimmed);
      }
    }
  }

  /**
   * Sends a JSON-RPC request to the MCP server.
   */
  async sendRequest(method: string, params?: any): Promise<any> {
    return new Promise((resolve, reject) => {
      const id = String(++this.requestId);
      const request = {
        jsonrpc: "2.0",
        id,
        method,
        params: params || {},
      };

      this.pendingRequests.set(id, { resolve, reject });

      if (this.process?.stdin?.writable) {
        this.process.stdin.write(JSON.stringify(request) + "\n");
      } else {
        this.pendingRequests.delete(id);
        reject(new Error("MCP server not connected"));
      }

      // Timeout after 30 seconds
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`Request '${method}' timed out`));
        }
      }, 30000);
    });
  }

  /**
   * Disconnects from the MCP server.
   */
  async disconnect(): Promise<void> {
    if (this.process) {
      this.process.kill();
      this.process = null;
    }
    this.pendingRequests.clear();
  }

  /**
   * Gets the path to the Rust binary.
   */
  static getBinaryPath(): string {
    // In development, the binary is in core/target/release/
    // In production (VS Code extension), it's in extension/bin/
    const possiblePaths = [
      path.resolve(__dirname, "..", "..", "core", "target", "release", "spire-rust"),
      path.resolve(__dirname, "..", "bin", "spire-rust"),
    ];

    for (const p of possiblePaths) {
      try {
        require("fs").accessSync(p);
        return p;
      } catch {
        continue;
      }
    }

    // Default to extension/bin/
    return path.resolve(__dirname, "..", "bin", "spire-rust");
  }
}
