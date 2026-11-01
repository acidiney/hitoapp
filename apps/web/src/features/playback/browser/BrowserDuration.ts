import type { MediaPort } from "../model";

const OVERLONG_RATIO = 1.1;
const OVERLONG_SECONDS = 5;
const END_EPSILON_SECONDS = 0.25;
const STALL_WINDOW_SECONDS = 2;
const STALL_MS = 2500;

export type ReconciledDuration = Readonly<{
  seconds: number;
  server: boolean;
}>;

export function reconcileDuration(
  media: number,
  serverMs: number | undefined,
): ReconciledDuration {
  const known = Number.isFinite(media) && media >= 0;
  if (!known) return { seconds: 0, server: false };
  const server = serverMs !== undefined && serverMs > 0 ? serverMs / 1000 : 0;
  const overlong =
    server > 0 &&
    media > server * OVERLONG_RATIO &&
    media - server > OVERLONG_SECONDS;
  return overlong
    ? { seconds: server, server: true }
    : { seconds: media, server: false };
}

export class BrowserEndGuard {
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(private end: () => void) {}

  observe(media: MediaPort, bound: number | null) {
    this.cancel();
    if (bound === null || media.paused || media.seeking) return;
    const position = media.currentTime;
    if (position >= bound - END_EPSILON_SECONDS) {
      this.end();
      return;
    }
    if (position < bound - STALL_WINDOW_SECONDS) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!media.paused && !media.seeking && media.currentTime === position)
        this.end();
    }, STALL_MS);
  }

  cancel() {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}
