import type { BrowserContext } from "@playwright/test";

export async function audiobookPlaybackDiagnostics(
  context: BrowserContext,
  origin: string,
) {
  let phase = 0;
  let timeOrigin = 0;
  let overflow = 0;
  const checkpoints: number[][] = [];
  const relative = (time: number) =>
    Number.isFinite(time) &&
    timeOrigin > 0 &&
    time >= timeOrigin &&
    time - timeOrigin <= 180_000
      ? Math.round(time - timeOrigin)
      : -1;
  return {
    phase: () => phase,
    relative,
    async arm() {
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const page = context
          .pages()
          .find((page) => new URL(page.url()).origin === origin);
        if (!page) throw new Error("The owned diagnostic page was unavailable");
        const value = await Promise.race([
          page.evaluate(() => performance.timeOrigin),
          new Promise<never>((_, reject) => {
            timer = setTimeout(
              () => reject(new Error("The owned diagnostic timed out")),
              2000,
            );
          }),
        ]);
        if (!Number.isFinite(value) || value <= 0)
          throw new Error("The owned diagnostic clock was unavailable");
        timeOrigin = value;
      } catch {
        overflow = 1;
      } finally {
        clearTimeout(timer);
      }
    },
    async checkpoint(
      value: number,
      failures: number,
      bodyFailures: number,
      active: number,
    ) {
      phase = value;
      if (checkpoints.length < 16)
        checkpoints.push([
          phase,
          relative(Date.now()),
          failures,
          bodyFailures,
          active,
        ]);
      else overflow = 1;
    },
    overflow: () => overflow,
    checkpoints: () => checkpoints.slice(),
  };
}
