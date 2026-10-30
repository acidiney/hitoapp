import { expect, test } from "vitest";
import { COVER_PALETTE } from "../ui/coverPalette";
import { contrast, rule, stylesheet, tokens } from "../../test/css";

test("cover text keeps 4.5:1 against the cover colour", () => {
  for (const entry of COVER_PALETTE)
    expect(contrast(entry.foreground, entry.background)).toBeGreaterThanOrEqual(
      4.5,
    );
});

test("cover shapes stay out of the title block", () => {
  const css = stylesheet("primitives.css");
  const circle = rule(css, '.fallback-cover[data-shape="circle"]::before');
  const top = parseFloat(circle.match(/top:\s*([\d.]+)%/)![1]);
  const size = parseFloat(circle.match(/width:\s*([\d.]+)%/)![1]);
  expect(top + size).toBeLessThanOrEqual(25);
  const band = rule(css, '.fallback-cover[data-shape="band"]::before');
  const width = parseFloat(band.match(/width:\s*([\d.]+)%/)![1]);
  expect(width).toBeLessThanOrEqual(9);
});

test("a switch that is off is at least 3:1 against its surroundings in both schemes", () => {
  const { light, dark } = tokens();
  for (const scheme of [light, dark]) {
    for (const ground of [scheme["--surface"], scheme["--bg"]])
      expect(contrast(scheme["--control-off"], ground)).toBeGreaterThanOrEqual(
        3,
      );
    expect(
      contrast(scheme["--ink"], scheme["--surface"]),
    ).toBeGreaterThanOrEqual(3);
  }
});

test("input and search borders are at least 3:1 in both schemes", () => {
  const { light, dark } = tokens();
  for (const scheme of [light, dark])
    for (const ground of [scheme["--surface"], scheme["--bg"]])
      expect(
        contrast(scheme["--control-border"], ground),
      ).toBeGreaterThanOrEqual(3);
});

test("primary and action buttons keep their label visible on hover", () => {
  const { light, dark } = tokens();
  const css = stylesheet("base.css");
  for (const variant of ["primary", "action"]) {
    const hover = rule(css, `.button.${variant}:hover:not(:disabled)`);
    expect(hover).not.toMatch(/inherit/);
    expect(hover).toMatch(
      new RegExp(
        `background:\\s*var\\(--${variant === "primary" ? "ink" : "action"}-hover\\)`,
      ),
    );
  }
  for (const [scheme, label] of [
    [light, "light"],
    [dark, "dark"],
  ] as const) {
    expect(
      contrast(scheme["--ink-hover"], scheme["--on-ink"]),
      `${label} ink hover`,
    ).toBeGreaterThanOrEqual(4.5);
    expect(
      contrast(scheme["--action-hover"], scheme["--on-action"]),
      `${label} action hover`,
    ).toBeGreaterThanOrEqual(4.5);
  }
});

test("the transport play button shows hover feedback that works on a light ink in dark mode", () => {
  const css = stylesheet("player.css");
  expect(rule(css, ".play-button:hover:not(:disabled)")).toMatch(
    /background:\s*var\(--ink-hover\)/,
  );
});

test("the playing cover and playlist swatches carry their own edge so dark covers show on a dark page", () => {
  const css = stylesheet("primitives.css") + stylesheet("shell.css");
  expect(rule(css, ".fallback-cover")).toMatch(/box-shadow:\s*inset/);
  expect(rule(css, ".playlist-swatch")).toMatch(/box-shadow:\s*inset/);
});

test("the mini player hides items by class, not by label", () => {
  expect(stylesheet("responsive.css")).not.toMatch(/\[aria-label/);
});

test("base styles declare the body once", () => {
  const css = stylesheet("base.css");
  expect(css.match(/^body\s*{/gm)).toHaveLength(1);
});
