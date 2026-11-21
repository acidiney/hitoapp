import type { Page } from "@playwright/test";

export async function settled(page: Page) {
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter(
          (animation) =>
            animation.effect?.getComputedTiming().endTime !== Infinity,
        )
        .map((animation) => animation.finished.catch(() => undefined)),
    ),
  );
}

const changeStreamPath = "/api/v1/changes/events";

export async function networkQuiet(page: Page, timeoutMs = 30_000) {
  const pending = new Set<unknown>();
  let lastActivity = Date.now();
  const touch = () => {
    lastActivity = Date.now();
  };
  const started = (request: { url(): string }) => {
    if (new URL(request.url()).pathname === changeStreamPath) return;
    pending.add(request);
    touch();
  };
  const finished = (request: unknown) => {
    pending.delete(request);
    touch();
  };
  page.on("request", started);
  page.on("requestfinished", finished);
  page.on("requestfailed", finished);
  const deadline = Date.now() + timeoutMs;
  try {
    while (pending.size > 0 || Date.now() - lastActivity < 500) {
      if (Date.now() > deadline)
        throw new Error("The page did not become network quiet");
      await page.waitForTimeout(50);
    }
  } finally {
    page.off("request", started);
    page.off("requestfinished", finished);
    page.off("requestfailed", finished);
  }
}
