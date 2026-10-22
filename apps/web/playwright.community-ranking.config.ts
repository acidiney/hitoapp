import { defineConfig } from "@playwright/test";

const browserName = process.env.HITO_RANKING_BROWSER ?? "chromium";
if (browserName !== "chromium" && browserName !== "webkit")
  throw new Error("The owned ranking gate requires Chromium or WebKit");

export default defineConfig({
  testDir: "./e2e",
  testMatch: "community-ranking.spec.ts",
  outputDir: "/evidence/playwright",
  workers: 1,
  retries: 0,
  timeout: 120_000,
  expect: { timeout: 10_000 },
  use: {
    browserName,
    baseURL: "http://localhost:8080",
    headless: true,
    viewport: { width: 1440, height: 1000 },
    trace: "off",
    screenshot: "off",
    video: "off",
    actionTimeout: 10_000,
    navigationTimeout: 10_000,
    launchOptions: {
      args:
        browserName === "chromium"
          ? ["--autoplay-policy=no-user-gesture-required"]
          : [],
    },
  },
  reporter: [["list"], ["json", { outputFile: "/evidence/result.json" }]],
});
