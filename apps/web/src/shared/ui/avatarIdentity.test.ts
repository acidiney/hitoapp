import { expect, test } from "vitest";
import { COVER_PALETTE } from "./coverPalette";
import { avatarColours, contrastRatio, initials } from "./avatarIdentity";

test("initials use the first letter of one name or the first two parts", () => {
  expect(initials("alice")).toBe("A");
  expect(initials("maria.silva")).toBe("MS");
  expect(initials("joao_pedro_costa")).toBe("JP");
  expect(initials("  ana  maria ")).toBe("AM");
  expect(initials("émile")).toBe("É");
  expect(initials("")).toBe("?");
  expect(initials("---")).toBe("?");
});

test("the colour of an account never changes between calls", () => {
  expect(avatarColours("7d2b0f0e-aa00")).toEqual(
    avatarColours("7d2b0f0e-aa00"),
  );
});

test("colours come from the shared palette and spread across accounts", () => {
  const ids = Array.from({ length: 60 }, (_, index) => `account-${index}`);
  const used = new Set(ids.map((id) => avatarColours(id).background));
  const allowed = new Set<string>(
    COVER_PALETTE.map((entry) => entry.background),
  );
  for (const background of used) expect(allowed.has(background)).toBe(true);
  expect(used.size).toBeGreaterThanOrEqual(5);
});

test("every avatar colour keeps its initials readable", () => {
  for (const entry of COVER_PALETTE) {
    const ratio = contrastRatio(entry.foreground, entry.background);
    expect(ratio).toBeGreaterThanOrEqual(4.5);
  }
  expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 0);
});

test("initials come from a display name and fall back to its first symbol", () => {
  expect(initials("Ana Souza")).toBe("AS");
  expect(initials("Zoë Müller")).toBe("ZM");
  expect(initials("宇多田ヒカル")).toBe("宇");
  expect(initials("🦉")).toBe("🦉");
  expect(initials("👩🏽‍🎤 Star")).toBe("S");
  expect(initials("👩🏽‍🎤")).toBe("👩🏽‍🎤");
  expect(initials("<b>")).toBe("B");
});
