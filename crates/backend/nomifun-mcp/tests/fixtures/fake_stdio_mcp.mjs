// A minimal MCP server over stdio, for the connector-call tests.
//
// Why a script and not a shell one-liner: the pooling tests need a server that
// stays alive across calls and can *report* what it saw, and the existing stdio
// fixtures in this crate are `#[cfg(unix)]` (`sh -c …`), which cannot run on a
// Windows dev box. Bun is already a hard dependency of this repository, so a
// script is the one fixture shape that speaks real JSON-RPC on every platform.
//
// It appends one line per event to `$FAKE_MCP_LOG`, which is how a test proves
// a session was *reused* rather than reopened: `initialize` appears once per
// spawned process.
//
//   start:<pid>            process came up
//   initialize:<pid>       completed the handshake
//   tools/list:<pid>       answered a tools/list
//   call:<pid>:<tool>      received a tools/call
//
// Tools:
//   tools/list            → two tools: `echo` (with an `inputSchema`) and `bare`
//                           (deliberately without one), so a caller can pin
//                           "the schema is the server's own, verbatim" and
//                           "no schema stays absent" against a real handshake
//   anything              → a normal result
//   boom                  → a tool-level failure (`isError: true`)
//   rpc-error             → a JSON-RPC error response
//   hang                  → no reply at all (drives the timeout path)
//   die                   → exits without replying (drives the broken-pipe path)
//   stubborn              → replies, then stops reading stdin and starts
//                           appending to `$FAKE_MCP_HB` forever. Closing the
//                           pipes cannot end it, so it stays alive unless it is
//                           actually killed — that is how the tests tell
//                           "reaped" apart from "exited on EOF".

import { appendFileSync } from "node:fs";
import { createInterface } from "node:readline";

const logPath = process.env.FAKE_MCP_LOG;
const note = (line) => {
  if (logPath) appendFileSync(logPath, `${line}\n`);
};

const reply = (message) => {
  process.stdout.write(`${JSON.stringify(message)}\n`);
};

note(`start:${process.pid}`);

const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;

  let message;
  try {
    message = JSON.parse(trimmed);
  } catch {
    return;
  }

  if (message.method === "initialize") {
    note(`initialize:${process.pid}`);
    reply({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: "2025-11-25",
        capabilities: { tools: {} },
        serverInfo: { name: "fake-stdio-mcp", version: "1.0.0" },
      },
    });
    return;
  }

  if (message.method === "notifications/initialized") return;

  // A real MCP server answers `tools/list`; this one does too, because the
  // connector tool read face (doc 26 §5) is only proven against a handshake
  // that carries an `inputSchema` the test did not invent on the wire.
  if (message.method === "tools/list") {
    note(`tools/list:${process.pid}`);
    reply({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        tools: [
          {
            name: "echo",
            description: "echo the arguments back",
            inputSchema: {
              type: "object",
              properties: { message: { type: "string", description: "text to echo" } },
              required: ["message"],
            },
          },
          // `inputSchema` is optional in the protocol, so the fixture publishes
          // one tool without it: "the server declared no schema" and "the server
          // declared an empty one" must stay distinguishable to a caller.
          { name: "bare", description: "declares no schema" },
        ],
      },
    });
    return;
  }

  if (message.method === "tools/call") {
    const tool = message.params?.name;
    note(`call:${process.pid}:${tool}`);

    if (tool === "hang") return;

    // Exits mid-call: the caller sees EOF rather than a reply, which is the
    // session-is-now-unknown case (no timing involved).
    if (tool === "die") process.exit(1);

    if (tool === "boom") {
      reply({
        jsonrpc: "2.0",
        id: message.id,
        result: {
          isError: true,
          content: [{ type: "text", text: "tool level failure" }],
        },
      });
      return;
    }

    if (tool === "rpc-error") {
      reply({ jsonrpc: "2.0", id: message.id, error: { code: -32602, message: "bad tool" } });
      return;
    }

    const args = message.params?.arguments ?? {};
    reply({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        content: [{ type: "text", text: `echo:${JSON.stringify(args)}` }],
        structuredContent: { saw: args },
      },
    });

    if (tool === "stubborn") {
      // Stop observing stdin so the pipe closing is not mistaken for a reason
      // to exit, and keep the event loop busy so nothing else ends it either.
      process.stdin.pause();
      const heartbeat = process.env.FAKE_MCP_HB;
      if (heartbeat) setInterval(() => appendFileSync(heartbeat, "."), 40);
    }
    return;
  }

  // Never leave a request unanswered: a silent unknown method would look like
  // a hung server and turn a test failure into a timeout.
  if (message.id !== undefined) {
    reply({
      jsonrpc: "2.0",
      id: message.id,
      error: { code: -32601, message: `unknown method ${message.method}` },
    });
  }
});
