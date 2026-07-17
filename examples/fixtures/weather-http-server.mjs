#!/usr/bin/env node
// Real Streamable HTTP MCP fixture server exposing a `get_weather` tool.
//
// Optionally auth-protected via env vars (checked BEFORE dispatch, incl. the
// initialize POST):
//   REQUIRE_BEARER=<token>                      -> requires `Authorization: Bearer <token>`
//   REQUIRE_API_KEY_HEADER + REQUIRE_API_KEY_VALUE -> requires that header == value
// Prints one line `{"port":<n>}` to stdout once listening (so tests can capture
// the ephemeral port). PORT env overrides the port (0 = ephemeral).
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import { z } from "zod";

const WEATHER = { Melbourne: "Sunny, 22C", Sydney: "Cloudy, 19C" };

function buildServer() {
  const server = new McpServer({ name: "weather-http-fixture", version: "1.0.0" });
  server.registerTool(
    "get_weather",
    { title: "Get Weather", description: "Get the current weather for a city", inputSchema: { city: z.string() } },
    async ({ city }) => {
      const text = WEATHER[city];
      if (!text) return { isError: true, content: [{ type: "text", text: `Unknown city: ${city}` }] };
      return { content: [{ type: "text", text }], isError: false };
    }
  );
  return server;
}

const REQUIRE_BEARER = process.env.REQUIRE_BEARER;
const REQUIRE_API_KEY_HEADER = process.env.REQUIRE_API_KEY_HEADER;
const REQUIRE_API_KEY_VALUE = process.env.REQUIRE_API_KEY_VALUE;

function authOk(req) {
  if (REQUIRE_BEARER) {
    return req.headers["authorization"] === `Bearer ${REQUIRE_BEARER}`;
  }
  if (REQUIRE_API_KEY_HEADER) {
    return req.headers[REQUIRE_API_KEY_HEADER.toLowerCase()] === REQUIRE_API_KEY_VALUE;
  }
  return true;
}

// Session id -> transport.
const transports = {};

async function readBody(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  if (chunks.length === 0) return undefined;
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

const httpServer = createServer(async (req, res) => {
  if (!authOk(req)) {
    res.writeHead(401, { "Content-Type": "application/json", "WWW-Authenticate": "Bearer" });
    res.end(JSON.stringify({ error: "unauthorized" }));
    return;
  }

  const sessionId = req.headers["mcp-session-id"];
  let transport;

  if (req.method === "POST") {
    const body = await readBody(req);
    if (sessionId && transports[sessionId]) {
      transport = transports[sessionId];
    } else {
      // New session (initialize).
      transport = new StreamableHTTPServerTransport({
        sessionIdGenerator: () => randomUUID(),
        onsessioninitialized: (sid) => {
          transports[sid] = transport;
        },
      });
      transport.onclose = () => {
        if (transport.sessionId) delete transports[transport.sessionId];
      };
      const mcp = buildServer();
      await mcp.connect(transport);
    }
    await transport.handleRequest(req, res, body);
    return;
  }

  if ((req.method === "GET" || req.method === "DELETE") && sessionId && transports[sessionId]) {
    await transports[sessionId].handleRequest(req, res);
    return;
  }

  res.writeHead(400).end("bad request");
});

const port = process.env.PORT ? Number(process.env.PORT) : 0;
httpServer.listen(port, "127.0.0.1", () => {
  const addr = httpServer.address();
  process.stdout.write(JSON.stringify({ port: addr.port }) + "\n");
});
