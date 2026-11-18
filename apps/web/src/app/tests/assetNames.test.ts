import { expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { assetFileNames, oneStylesheet } from "../../../vite.assets";

test("font files get content-hashed names so an update is never served stale", () => {
  expect(assetFileNames({ names: ["figtree-latin-wght-normal.woff2"] })).toBe(
    "assets/[name]-[hash][extname]",
  );
  expect(assetFileNames({ names: ["main.css"] })).toBe("assets/app.[ext]");
});

test("a build that splits the styles fails, because the server links only assets/app.css", () => {
  expect(() =>
    oneStylesheet({
      app: { fileName: "assets/app.js" },
      styles: { fileName: "assets/app.css" },
    }),
  ).not.toThrow();
  expect(() =>
    oneStylesheet({
      own: { fileName: "assets/app.css" },
      shared: { fileName: "assets/app2.css" },
    }),
  ).toThrow("assets/app.css, assets/app2.css");
});

test("the desktop first screen links the same stylesheet as the server's pages", () => {
  const page = readFileSync(
    resolve(__dirname, "../../../first-open.html"),
    "utf8",
  );
  expect(page).toContain('<link rel="stylesheet" href="/assets/app.css" />');
});
