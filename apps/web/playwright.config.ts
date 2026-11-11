import { defineConfig } from "@playwright/test";
import {
  adminSessionPath,
  sharedAdminSession,
} from "./e2e/fixtures/admin-session";
export default defineConfig({
  testDir: "./e2e",
  globalSetup: "./e2e/global-setup.ts",
  testIgnore: [
    "**/*.test.ts",
    "**/latency/**",
    "**/collection-artwork.spec.ts",
    "**/community-ranking.spec.ts",
    "**/plugin-authorization-callback.spec.ts",
    "**/plugin-connection-ui.spec.ts",
  ],
  workers: 1,
  retries: 0,
  timeout: 60000,
  use: {
    baseURL: process.env.HITO_TEST_URL ?? "http://e2e-server:8080",
    storageState: sharedAdminSession ? adminSessionPath : undefined,
    headless: true,
    serviceWorkers: "block",
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    launchOptions: { args: ["--autoplay-policy=no-user-gesture-required"] },
  },
  reporter: [["list"]],
});
