import { afterEach, beforeEach, vi, type MockInstance } from "vitest";

const automatic = /^hito:lyrics-auto:/;

export function setAutomaticLyrics(enabled: boolean) {
  const read = Storage.prototype.getItem;
  let spy: MockInstance | null = null;
  beforeEach(() => {
    spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(function (
      this: Storage,
      key: string,
    ) {
      return automatic.test(key)
        ? enabled
          ? null
          : "off"
        : read.call(this, key);
    });
  });
  afterEach(() => {
    spy?.mockRestore();
  });
}
