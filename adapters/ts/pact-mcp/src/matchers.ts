// Matcher helpers for the MCP plugin.
//
// IMPORTANT: these do NOT perform matching. They only produce the Pact
// matcher-definition DSL *strings* that the Rust engine's ConfigureInteraction
// parses (`matching(type, 'x')`, `matching(regex, '<re>', '<ex>')`, ...). All
// actual matching happens in the Rust engine. We can't re-export pact-js's own
// matchers here because those emit a different in-body marker format; the MCP
// content matcher expects the inline-DSL leaf strings.

const MATCHER = Symbol("mcpMatcher");

export interface McpMatcher {
  [MATCHER]: true;
  /** The inline Pact matcher-definition DSL string, e.g. `matching(type, 'x')`. */
  dsl: string;
}

export function isMcpMatcher(v: unknown): v is McpMatcher {
  return typeof v === "object" && v !== null && (v as Record<symbol, unknown>)[MATCHER] === true;
}

function matcher(dsl: string): McpMatcher {
  return { [MATCHER]: true, dsl };
}

/** Escape a value for single-quoted use inside the DSL. */
function q(value: string): string {
  return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
}

/** Match by type (any value of the same JSON type as `example`). Pact's `like`. */
export function like(example: string | number | boolean): McpMatcher {
  if (typeof example === "number") return matcher(`matching(number, ${example})`);
  if (typeof example === "boolean") return matcher(`matching(boolean, ${example})`);
  return matcher(`matching(type, ${q(example)})`);
}

/** Match a string against a regular expression. */
export function regex(pattern: string, example: string): McpMatcher {
  return matcher(`matching(regex, ${q(pattern)}, ${q(example)})`);
}

/** Match any number (integer or decimal), example numeric value. */
export function number(example: number): McpMatcher {
  return matcher(`matching(number, ${example})`);
}

/** Match any integer. */
export function integer(example: number): McpMatcher {
  return matcher(`matching(integer, ${Math.trunc(example)})`);
}

/** Match any boolean. */
export function boolean(example: boolean): McpMatcher {
  return matcher(`matching(boolean, ${example})`);
}

/** Match any non-empty string. */
export function notEmpty(example: string): McpMatcher {
  return matcher(`notEmpty(${q(example)})`);
}

/**
 * Recursively convert a value tree containing `McpMatcher`s into the plain
 * JSON the plugin expects: each matcher becomes its inline-DSL string leaf.
 */
export function buildDsl(value: unknown): unknown {
  if (isMcpMatcher(value)) return value.dsl;
  if (Array.isArray(value)) return value.map(buildDsl);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = buildDsl(v);
    }
    return out;
  }
  return value;
}
