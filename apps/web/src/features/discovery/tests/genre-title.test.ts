import { expect, test } from "vitest";
import { genreTitle } from "../model/genreTitle";

test.each([
  ["pop soul", "Pop Soul"],
  ["folk rock", "Folk Rock"],
  ["afrobeats", "Afrobeats"],
  ["j-pop", "J-Pop"],
  ["k-pop", "K-Pop"],
  ["post-punk revival", "Post-Punk Revival"],
  ["r&b", "R&B"],
  ["contemporary r&b", "Contemporary R&B"],
  ["edm", "EDM"],
  ["uk garage", "UK Garage"],
  ["us rap", "US Rap"],
  ["dj mix", "DJ Mix"],
  ["mpb", "MPB"],
  ["drum & bass", "Drum & Bass"],
  ["Fado", "Fado"],
  ["AOR", "AOR"],
  ["Hip Hop", "Hip Hop"],
  ["", ""],
])("genreTitle(%j) is %j", (label, title) => {
  expect(genreTitle(label)).toBe(title);
});
