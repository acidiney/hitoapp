import { defineConfig } from "@playwright/test";

const browserName = process.env.HITO_ARTWORK_BROWSER ?? "chromium";
if (browserName !== "chromium" && browserName !== "webkit")
  throw new Error("The owned artwork gate requires Chromium or WebKit");

export default defineConfig({
  testDir: "./e2e",
  testMatch: "collection-artwork.spec.ts",
  outputDir: "/evidence/playwright",
  workers: 1,
  retries: 0,
  timeout: 180_000,
  expect: { timeout: 15_000 },
  use: {
    browserName,
    baseURL: process.env.HITO_TEST_URL,
    headless: true,
    viewport: { width: 1440, height: 1000 },
    screenshot: "only-on-failure",
    trace: "off",
    actionTimeout: 15_000,
    navigationTimeout: 15_000,
  },
  reporter: [["list"], ["json", { outputFile: "/evidence/result.json" }]],
});
