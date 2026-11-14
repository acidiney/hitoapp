import { expect, test, vi } from "vitest";
import { ArtworkRetrieval } from "../api/artworkRetrieval";

const target = { kind: "playlist" as const, id: "p1" };

test("a changed cover reloads its visible image under a new revision", () => {
  const retrieval = new ArtworkRetrieval("alice");
  const listener = vi.fn();
  retrieval.subscribe(target, listener);
  const before = retrieval.snapshot(target).revision;
  retrieval.refresh(target);
  const after = retrieval.snapshot(target);
  expect(after.revision).toBeGreaterThan(before);
  expect(after.imageFailed).toBe(false);
  expect(listener).toHaveBeenCalledTimes(1);
  retrieval.refresh(target);
  expect(retrieval.snapshot(target).revision).toBeGreaterThan(after.revision);
});

test("a changed cover nobody is showing is forgotten without work", () => {
  const retrieval = new ArtworkRetrieval("alice");
  retrieval.refresh(target);
  expect(retrieval.snapshot(target).revision).toBe(0);
});

test("a closed retrieval ignores changed covers", () => {
  const retrieval = new ArtworkRetrieval("alice");
  const listener = vi.fn();
  retrieval.subscribe(target, listener);
  retrieval.close();
  retrieval.refresh(target);
  expect(listener).not.toHaveBeenCalled();
});
