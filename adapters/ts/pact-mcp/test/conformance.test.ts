import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { runCompare } from "../src/index";

// Anti-divergence gate (spec §4.3): the TS adapter runs the SAME golden fixtures
// as the Rust engine, by driving the engine (compare CLI). Matching stays in
// Rust; this asserts the README contract (match + set of mismatchPaths).
const conformanceDir = join(__dirname, "..", "..", "..", "..", "docs", "spec", "conformance");

const fixtures = readdirSync(conformanceDir).filter((f) => f.endsWith(".json"));

describe("conformance fixtures (driven through the Rust engine)", () => {
  it("finds the fixtures", () => {
    expect(fixtures.length).toBeGreaterThan(0);
  });

  for (const file of fixtures) {
    it(file, () => {
      const fixture = JSON.parse(readFileSync(join(conformanceDir, file), "utf8"));
      const result = runCompare(join(conformanceDir, file));

      expect(result.match).toBe(fixture.expected.match);
      const expectedPaths = [...(fixture.expected.mismatchPaths ?? [])].sort();
      expect([...result.mismatchPaths].sort()).toEqual(expectedPaths);
    });
  }
});
