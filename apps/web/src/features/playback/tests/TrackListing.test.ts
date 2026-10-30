import { expect, test, vi } from "vitest";
import { MAX_QUEUE_ENTRIES, type Track } from "../model";
import { LISTING_PAGE, listingSupply, trackListing } from "../api/trackListing";

const track = (index: number): Track => ({
  id: `r${index}`,
  title: `Track ${index}`,
  artist: "Fixture",
  duration_ms: 1000,
  available: true,
});

function listing(total: number, offset = 0) {
  const fetch = vi.fn(
    async (from: number, limit: number, signal: AbortSignal) => {
      signal.throwIfAborted();
      return {
        total,
        items: Array.from(
          { length: Math.max(0, Math.min(limit, total - from)) },
          (_, i) => track(from + i),
        ),
      };
    },
  );
  return { total, offset, fetch };
}

test("pages are requested in order from the first unloaded position", async () => {
  const source = listing(450);
  const supply = listingSupply(source, 200);
  expect(supply.remaining()).toBe(250);
  const signal = new AbortController().signal;
  expect(
    (await supply.next(signal)).map((t) => ("id" in t ? t.id : ""))[0],
  ).toBe("r200");
  expect(supply.remaining()).toBe(50);
  expect(await supply.next(signal)).toHaveLength(50);
  expect(supply.remaining()).toBe(0);
  expect(source.fetch.mock.calls.map((call) => [call[0], call[1]])).toEqual([
    [200, LISTING_PAGE],
    [400, LISTING_PAGE],
  ]);
});

test("a list whose total changes mid-load stops with an explanation instead of skipping or repeating", async () => {
  for (const changed of [500, 1500]) {
    const source = {
      total: 1000,
      offset: 0,
      fetch: vi.fn().mockResolvedValue({
        total: changed,
        items: Array.from({ length: 200 }, (_, i) => track(200 + i)),
      }),
    };
    const supply = listingSupply(source, 200);
    await expect(
      supply.next(new AbortController().signal),
    ).rejects.toMatchObject({
      message:
        "The list changed while loading. Play it again to include the rest.",
    });
  }
});

test("an empty page ends the list", async () => {
  const source = {
    total: 1000,
    offset: 0,
    fetch: vi.fn().mockResolvedValue({ total: 1000, items: [] }),
  };
  const supply = listingSupply(source, 200);
  expect(await supply.next(new AbortController().signal)).toEqual([]);
  expect(supply.remaining()).toBe(0);
});

test("the supply never promises more than the queue limit", () => {
  const supply = listingSupply(listing(MAX_QUEUE_ENTRIES + 50_000), 200);
  expect(supply.remaining()).toBe(MAX_QUEUE_ENTRIES - 200);
});

test("an aborted fetch rejects and is not repeated", async () => {
  const source = listing(400);
  const supply = listingSupply(source, 200);
  const controller = new AbortController();
  controller.abort();
  await expect(supply.next(controller.signal)).rejects.toBeTruthy();
  expect(source.fetch).toHaveBeenCalledTimes(0);
});

test("a listing carries the page position and fetches through the given reader", async () => {
  const source = listing(450, 200);
  const built = trackListing({ total: 450, offset: 200 }, source.fetch);
  expect(built.total).toBe(450);
  expect(built.offset).toBe(200);
  const page = await built.fetch(0, 2, new AbortController().signal);
  expect(page.items.map((item) => item.id)).toEqual(["r0", "r1"]);
});
