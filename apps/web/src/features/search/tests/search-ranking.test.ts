import { expect, test } from "vitest";
import { topResultKind } from "../model/ranking";

test("an exact chosen or library artist is the top result even when a track also matches", () => {
  expect(
    topResultKind("joeboy", {
      tracks: 3,
      releases: 1,
      artistNames: ["Joéboy", "Joeboy and friends"],
    }),
  ).toBe("artist");
  expect(
    topResultKind("joe", { tracks: 3, releases: 1, artistNames: ["Joeboy"] }),
  ).toBe("track");
  expect(
    topResultKind("joe", { tracks: 0, releases: 1, artistNames: ["Joeboy"] }),
  ).toBe("release");
  expect(
    topResultKind("joe", { tracks: 0, releases: 0, artistNames: ["Joeboy"] }),
  ).toBe("artist");
  expect(
    topResultKind("joe", { tracks: 0, releases: 0, artistNames: [] }),
  ).toBeNull();
});
