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

/**
 * Applies the chosen speed to the current media and verifies what the browser
 * actually accepted. A refusal leaves the element at unit speed and reports a
 * unit-only source; the chosen speed stays with the caller so a later source
 * can present it.
 */
export function applyBrowserSpeed(
  media: MediaPort,
  chosen: number,
): BrowserSpeed {
  if (assign(media, chosen))
    return { effective: chosen, range: BROWSER_SPEED_RANGE };
  try {
    if (media.playbackRate !== 1) media.playbackRate = 1;
  } catch {
    // The element stays at its own rate; the reported speed is still 1×.
  }
  return { effective: 1, range: UNIT_ONLY };
}
