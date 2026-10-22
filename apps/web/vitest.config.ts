import { availableParallelism } from "node:os";
import { defineConfig } from "vitest/config";

const workerCeiling = 4;

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts", "e2e/**/*.test.ts"],
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"],
    maxWorkers: Math.min(workerCeiling, availableParallelism()),
  },
});
