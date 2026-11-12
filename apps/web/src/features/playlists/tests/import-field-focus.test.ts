import { expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const css = readFileSync(join(process.cwd(), "src/styles/import.css"), "utf8");

test("the import link field shows one focus indicator on its wrapper", () => {
  expect(css).toMatch(
    /\.import-link:focus-within\s*{[^}]*border-color: var\(--action\)/,
  );
  expect(css).not.toMatch(/\.import-link:focus-within\s*{[^}]*outline/);
  expect(css).toMatch(
    /\.import-link input:focus-visible\s*{\s*outline: none;\s*}/,
  );
});
