import { expect, test } from "vitest";
import { assetFileNames } from "../../../vite.assets";

test("font files get content-hashed names so an update is never served stale", () => {
  expect(assetFileNames({ names: ["figtree-latin-wght-normal.woff2"] })).toBe(
    "assets/[name]-[hash][extname]",
  );
  expect(assetFileNames({ names: ["main.css"] })).toBe("assets/app.[ext]");
});
