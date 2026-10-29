import { coverStyle } from "./coverPalette";

/**
 * Flat header colour for artist and playlist pages. The server does not extract
 * a dominant colour from artwork, so the colour is the generated-cover palette
 * entry for a stable id (the artist's first release, or the collection's own id).
 * The same id always gives the same colour, and every palette colour keeps white
 * text at readable contrast.
 */
export function tintFor(id: string): string {
  return coverStyle(id).background;
}
