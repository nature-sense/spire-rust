# Fix Plan: Transport Re-entrancy Deadlock

## Problem
The single line-processor task in `Transport` blocks when `call_extension` awaits a response, preventing it from processing the incoming response message. This causes a deadlock.

## Fix Strategy
Restructure the transport to use a **dedicated response receiver** per `call_extension` call, rather than relying on the line processor to deliver responses through the shared pending map.

### Changes needed:

1. **`spire-core/src/transport/stdio.rs`**: 
   - Add a `response_tx` broadcast channel alongside the line channel
   - When `call_extension` sends a request, it subscribes to the broadcast channel
   - The line processor publishes all incoming messages to the broadcast channel
   - `call_extension` uses `tokio::select!` to wait for either the matching response or timeout
   - This allows the line processor to continue processing other messages

2. **`spire-core/src/main.rs`**:
   - The request handler should not block the line processor
   - Instead of `block_in_place`, use a proper async approach

## Simpler Alternative
A simpler fix: Instead of having the request handler block the line processor, make the request handler spawn the coordinator task and return immediately. Then have the coordinator send the response back through a different mechanism.

Actually, the simplest fix: **Don't use `block_in_place` in the request handler**. Instead, make the request handler return a future. But the current `RequestHandler` type is `Fn(Value) -> Value` (synchronous).

**Best fix**: Change the architecture so that `call_extension` doesn't block the line processor. Instead of awaiting `response_rx` in the line processor task, we should:
1. Write the request to stdout
2. Return a future that can be awaited from the coordinator task
3. The line processor continues processing messages independently
4. When the response arrives, it's matched to the pending entry and delivered via the oneshot channel

This is actually the current design! The issue is that `call_extension` is called FROM the line processor task (via the request handler → coordinator → tool_router → call_extension chain).

**The real fix**: Break the call chain so that `call_extension` is not called from within the line processor task.
