import { defineConfig, type Project } from "@playwright/test";

const gateSpecs = {
  "collection-artwork": "collection-artwork.spec.ts",
  "community-ranking": "community-ranking.spec.ts",
  "lyric-provider-choice": "recording-lyric-provider-choice.spec.ts",
} as const;
export type Gate = keyof typeof gateSpecs;

const liveSpecs = "**/*-live.spec.ts";
const viewport = { width: 1440, height: 1000 };

function gateBrowser(variable: string, label: string) {
  const name = process.env[variable] ?? "chromium";
  if (name !== "chromium" && name !== "webkit")
    throw new Error(`The owned ${label} gate requires Chromium or WebKit`);
  return name;
}

function gateProject(gate: Gate): Project {
  const testMatch = gateSpecs[gate];
  switch (gate) {
    case "collection-artwork":
      return {
        name: gate,
        testMatch,
        outputDir: "/evidence/playwright",
        timeout: 180_000,
        expect: { timeout: 15_000 },
        use: {
          browserName: gateBrowser("HITO_ARTWORK_BROWSER", "artwork"),
          baseURL: process.env.HITO_TEST_URL,
          headless: true,
          viewport,
          screenshot: "only-on-failure",
          trace: "off",
          actionTimeout: 15_000,
          navigationTimeout: 15_000,
        },
      };
    case "community-ranking": {
      const browserName = gateBrowser("HITO_RANKING_BROWSER", "ranking");
      return {
        name: gate,
        testMatch,
        outputDir: "/evidence/playwright",
        timeout: 120_000,
        expect: { timeout: 10_000 },
        use: {
          browserName,
          baseURL: "http://localhost:8080",
          headless: true,
          viewport,
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
      };
    }
    case "lyric-provider-choice":
      return {
        name: gate,
        testMatch,
        timeout: 120_000,
        use: {
          baseURL: process.env.HITO_TEST_URL ?? "http://localhost:8080",
          headless: true,
          viewport,
          trace: "off",
          screenshot: "off",
          video: "off",
          launchOptions: { args: [] },
        },
      };
  }
}

const gateReporters = {
  "collection-artwork": [
    ["list"],
    ["json", { outputFile: "/evidence/result.json" }],
  ],
  "community-ranking": [
    ["list"],
    ["json", { outputFile: "/evidence/result.json" }],
  ],
  "lyric-provider-choice": [["list"]],
} as const;

const base = { testDir: "./e2e", workers: 1, retries: 0 };

export function gateConfig(gate: Gate) {
  return defineConfig({
    ...base,
    reporter: [...gateReporters[gate]],
    projects: [gateProject(gate)],
  });
}

const ordinary = {
  baseURL: process.env.HITO_TEST_URL ?? "http://e2e-server:8080",
  headless: true,
  viewport,
  screenshot: "only-on-failure",
  trace: "retain-on-failure",
  launchOptions: { args: ["--autoplay-policy=no-user-gesture-required"] },
} as const;

export default defineConfig({
  ...base,
  timeout: 60000,
  reporter: [["list"]],
  projects: [
    {
      name: "default",
      testIgnore: [liveSpecs, ...Object.values(gateSpecs), "**/*.test.ts"],
      use: ordinary,
    },
    { name: "live", testMatch: liveSpecs, use: ordinary },
    ...(Object.keys(gateSpecs) as Gate[]).map(gateProject),
  ],
});
