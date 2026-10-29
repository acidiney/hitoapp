import { expect, test } from "vitest";
import { COVER_PALETTE } from "../ui/coverPalette";
import { HEADER_MUTED_ALPHA, contrastOnTint } from "../ui/collectionTint";

test("header supporting text keeps 4.5:1 on every palette tint", () => {
  for (const entry of COVER_PALETTE)
    expect(
      contrastOnTint(entry.background, HEADER_MUTED_ALPHA),
    ).toBeGreaterThanOrEqual(4.5);
});

test("plain white keeps 4.5:1 on every palette tint", () => {
  for (const entry of COVER_PALETTE)
    expect(contrastOnTint(entry.background, 1)).toBeGreaterThanOrEqual(4.5);
});
