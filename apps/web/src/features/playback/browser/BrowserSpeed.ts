import type { MediaPort, SpeedRange } from "../model";

export const BROWSER_SPEED_RANGE: SpeedRange = { min: 0.5, max: 3 };
const UNIT_ONLY: SpeedRange = { min: 1, max: 1 };

export type BrowserSpeed = { effective: number; range: SpeedRange };

function assign(media: MediaPort, rate: number): boolean {
  try {
    if (media.defaultPlaybackRate !== rate) media.defaultPlaybackRate = rate;
    if (media.playbackRate !== rate) media.playbackRate = rate;
    return media.playbackRate === rate;
  } catch {
    return false;
  }
}

export function applyBrowserSpeed(
  media: MediaPort,
  chosen: number,
): BrowserSpeed {
  if (assign(media, chosen))
    return { effective: chosen, range: BROWSER_SPEED_RANGE };
  assign(media, 1);
  return { effective: 1, range: UNIT_ONLY };
}
