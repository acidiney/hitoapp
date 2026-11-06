import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "@playwright/test";
import { outputDirectory } from "./results";

test("render the latency distribution chart", async ({ page }) => {
  const size = JSON.parse(
    readFileSync(join(outputDirectory, "chart-size.json"), "utf8"),
  ) as { width: number; height: number };
  await page.setViewportSize({ width: size.width, height: size.height });
  await page.setContent(
    readFileSync(join(outputDirectory, "chart.html"), "utf8"),
  );
  await page.screenshot({
    path: join(outputDirectory, "latency-distribution.png"),
  });
});
