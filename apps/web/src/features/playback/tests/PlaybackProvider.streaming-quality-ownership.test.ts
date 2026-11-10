import { expect, test, vi } from "vitest";
import { browserPreparationMedia } from "../fixtures/browserPreparationMedia";
import { renderAccount } from "../fixtures/browserMediaSessionPlayback";

test("Strict Mode acquires browser listeners only for its committed owner and unmount removes every quality callback", () => {
  const media = browserPreparationMedia();
  const added = vi.spyOn(window, "addEventListener");
  const removed = vi.spyOn(window, "removeEventListener");
  const mediaAdded = vi.spyOn(HTMLMediaElement.prototype, "addEventListener");
  const view = renderAccount(true);
  const discarded = media.created.filter((element) => !element.isConnected);
  const qualityEvents = new Set(["hito:stream-quality-changed", "storage"]);
  const registered = added.mock.calls.filter(([event]) =>
    qualityEvents.has(event),
  );
  const abandonedMediaCallbacks = mediaAdded.mock.calls.filter((_call, index) =>
    discarded.includes(mediaAdded.mock.contexts[index] as HTMLAudioElement),
  );
  view.unmount();
  try {
    expect(discarded.length).toBeGreaterThan(0);
    expect(registered.length).toBeGreaterThan(0);
    expect(
      registered.filter(
        ([event, listener]) =>
          !removed.mock.calls.some(
            ([removedEvent, removedListener]) =>
              removedEvent === event && removedListener === listener,
          ),
      ),
    ).toEqual([]);
    expect(abandonedMediaCallbacks).toEqual([]);
  } finally {
    view.unmount();
    media.close();
  }
});
