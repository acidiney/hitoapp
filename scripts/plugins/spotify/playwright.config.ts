import { defineConfig } from "/workspace/apps/web/node_modules/@playwright/test";

export default defineConfig({
  testDir: "/workspace/scripts/plugins/spotify/e2e",
  outputDir: "/workspace/test-results/plugins-spotify/playwright",
  workers: 1,
  retries: 0,
  timeout: 120000,
  expect: { timeout: 10000 },
  use: {
    baseURL: "http://localhost:8080",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    actionTimeout: 15000,
    navigationTimeout: 20000,
    trace: "retain-on-failure",
    screenshot: "off",
    video: "off",
  },
  reporter: [["list"]],
});
