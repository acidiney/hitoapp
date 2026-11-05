import { defineConfig, devices } from "@playwright/test";

const callback = "plugin-authorization-callback.spec.ts";
const connection = "plugin-connection-ui.spec.ts";

export default defineConfig({
  testDir: "./e2e",
  workers: 2,
  retries: 0,
  timeout: 15 * 60_000,
  reporter: [["list"]],
  use: {
    baseURL: process.env.HITO_TEST_URL ?? "http://127.0.0.1:8080",
    headless: true,
    viewport: { width: 900, height: 600 },
    trace: "off",
    video: process.env.HITO_PLUGIN_AUTHORIZATION_VIDEO ? "on" : "off",
  },
  projects: [
    {
      name: "chromium",
      testMatch: callback,
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "webkit",
      testMatch: callback,
      use: { ...devices["Desktop Safari"] },
    },
    {
      name: "chromium-connection",
      testMatch: connection,
      dependencies: ["chromium", "webkit"],
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "webkit-connection",
      testMatch: connection,
      dependencies: ["chromium", "webkit"],
      use: { ...devices["Desktop Safari"] },
    },
  ],
});
