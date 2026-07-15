// ──────────────────────────────────────────────
// Actor Roles — must match core/src/api/actor_ref.rs::roles
// ──────────────────────────────────────────────

/**
 * Well-known actor roles in the Spire actor system.
 *
 * These constants must be kept in sync with the Rust `roles` module
 * in `core/src/api/actor_ref.rs`.
 */
export const ACTOR_ROLES = {
  // ── Rust-side actors ──
  COORDINATOR: 'coordinator',
  MCP_MANAGER: 'mcp-manager',
  GENERALIST_AGENT: 'generalist-agent',
  PLANNER_AGENT: 'planner-agent',
  LLM: 'llm',
  MEMORY_GRAPH: 'memory-graph',
  EMBEDDER: 'embedder',
  PROGRESS: 'progress',
  TOOLS_MANAGER: 'tools-manager',
  TS_BRIDGE: 'ts-bridge',

  // ── TS-side actors ──
  CHAT_SERVICE: 'chat-service',
  CONFIG_SERVICE: 'config-service',
  TOOL_REGISTRY: 'tool-registry',
  SIDEBAR: 'sidebar',
  EVENT_BUS: 'event-bus',
} as const;

/** Union type of all actor role string values. */
export type ActorRole = typeof ACTOR_ROLES[keyof typeof ACTOR_ROLES];

/**
 * A reference to any actor in the system (local or remote).
 *
 * Mirrors the Rust `ActorRef` struct in `core/src/api/actor_ref.rs`.
 */
export interface ActorRef {
  /** Globally unique actor ID (auto-generated). */
  id: string;
  /** Well-known role name (e.g. "coordinator", "chat-service"). */
  role: ActorRole;
  /** Which runtime owns this actor: "rust" or "ts". */
  domain: 'rust' | 'ts';
}

/**
 * Check whether an actor reference belongs to the Rust runtime.
 */
export function isRustActor(ref: ActorRef): boolean {
  return ref.domain === 'rust';
}

/**
 * Check whether an actor reference belongs to the TS runtime.
 */
export function isTsActor(ref: ActorRef): boolean {
  return ref.domain === 'ts';
}

/**
 * Create a new ActorRef for a Rust-side actor.
 */
export function rustActor(id: string, role: ActorRole): ActorRef {
  return { id, role, domain: 'rust' };
}

/**
 * Create a new ActorRef for a TS-side actor.
 */
export function tsActor(id: string, role: ActorRole): ActorRef {
  return { id, role, domain: 'ts' };
}
