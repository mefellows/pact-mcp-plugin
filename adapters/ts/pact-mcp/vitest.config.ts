import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // pact-js FFI and the engine mock/plugin hold process-global native state;
    // run test files sequentially in a single worker to avoid cross-talk.
    fileParallelism: false,
    pool: "forks",
    poolOptions: { forks: { singleFork: true } },
    testTimeout: 30000,
    hookTimeout: 30000,
  },
});
