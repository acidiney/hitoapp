import { beforeEach, expect, test, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  window.name = "";
});

async function guard() {
  return import("../reloadGuard");
}

test("a reload is allowed once and then waits out the spacing", async () => {
  const { reloadWait, markReload } = await guard();
  expect(reloadWait(1_000_000)).toBe(0);
  markReload(1_000_000);
  expect(reloadWait(1_005_000)).toBe(10_000);
  expect(reloadWait(1_015_000)).toBe(0);
});

test("a page that loads successfully clears the limiter", async () => {
  const { reloadWait, markReload, markPageLoaded } = await guard();
  markReload(Date.now());
  expect(reloadWait()).toBeGreaterThan(0);
  markPageLoaded();
  expect(reloadWait()).toBe(0);
  expect(sessionStorage.getItem("hito-connection-reload")).toBeNull();
});

test("when session storage throws the limiter survives a reload through the window name", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  const first = await guard();
  first.markReload(Date.now());
  expect(first.reloadWait()).toBeGreaterThan(0);
  vi.resetModules();
  const second = await guard();
  expect(second.reloadWait()).toBeGreaterThan(0);
  second.markPageLoaded();
  expect(second.reloadWait()).toBe(0);
});

test("a window name that belongs to something else is left alone", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  window.name = "someone-else";
  const { markReload, reloadWait } = await guard();
  markReload(Date.now());
  expect(window.name).toBe("someone-else");
  expect(reloadWait()).toBeGreaterThan(0);
});
