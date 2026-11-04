import { defineConfig, devices } from "@playwright/test";
export default defineConfig({
  testDir: "./e2e",
  testMatch: "plugin-authorization-callback.spec.ts",
  workers: 2,
  retries: 0,
  timeout: 15 * 60_000,
  reporter: [["list"]],
  use: {
    baseURL: process.env.HITO_TEST_URL ?? "http://127.0.0.1:8080",
    headless: true,
    viewport: { width: 900, height: 600 },
    trace: "off",
    video: "off",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
