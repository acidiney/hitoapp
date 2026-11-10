import { defineConfig } from "@playwright/test";
import { adminSessionPath } from "./e2e/fixtures/admin-session";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "streaming-quality.spec.ts",
  globalSetup: "./e2e/global-setup.ts",
  workers: 1,
  retries: 0,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  projects: [
    {
      name: "chromium",
      grep: /@chromium-original-fallback/,
      use: { browserName: "chromium" },
    },
    { name: "webkit", grep: /@webkit-aac/, use: { browserName: "webkit" } },
  ],
  use: {
    baseURL: process.env.HITO_TEST_URL ?? "http://e2e-server:8080",
    storageState: adminSessionPath,
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  reporter: [["list"]],
});
