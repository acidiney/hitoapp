import { expect, test } from "vitest";
import { ownedRowState } from "../components/EditionTrackRow";

const recording = (available: boolean, services: boolean) =>
  ({ available, plays_from_services: services }) as Parameters<
    typeof ownedRowState
  >[0];

test("an owned playable row is Ready without claiming a local file", () => {
  expect(ownedRowState(recording(true, false))).toEqual({ state: "ready" });
  expect(ownedRowState(recording(true, true))).toEqual({ state: "ready" });
});

test("an owned row that only plays from services says so, and one without a source stays in the library", () => {
  expect(ownedRowState(recording(false, true))).toEqual({
    state: "ready",
    detail: "from your services",
  });
  expect(ownedRowState(recording(false, false))).toEqual({
    state: "library",
  });
});
