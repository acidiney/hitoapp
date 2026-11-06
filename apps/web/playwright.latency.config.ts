import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e/latency",
  testMatch: process.env.BENCH_CHART ? "chart.spec.ts" : "start.spec.ts",
  workers: 1,
  retries: 0,
  timeout: 4 * 60 * 60 * 1000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: process.env.HITO_TEST_URL ?? "http://e2e-server:8080",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "off",
    trace: "off",
    launchOptions: { args: ["--autoplay-policy=no-user-gesture-required"] },
  },
  reporter: [["list"]],
});
