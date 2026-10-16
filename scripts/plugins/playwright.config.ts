import { defineConfig } from "/workspace/apps/web/node_modules/@playwright/test";
export default defineConfig({
  testDir: "/workspace/scripts/plugins/e2e",
  outputDir: "/workspace/test-results/plugins-folder/playwright",
  workers: 1,
  retries: 0,
  timeout: 60000,
  use: {
    baseURL: "http://localhost:8080",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    launchOptions: { args: ["--autoplay-policy=no-user-gesture-required"] },
  },
  reporter: [["list"]],
});
