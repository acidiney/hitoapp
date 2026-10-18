import { vi } from "vitest";

export function browserMediaSession() {
  const original = Object.getOwnPropertyDescriptor(navigator, "mediaSession");
  const handlers = new Map<MediaSessionAction, MediaSessionActionHandler>();
  const session = {
    metadata: null as MediaMetadata | null,
    playbackState: "none" as MediaSessionPlaybackState,
    position: undefined as MediaPositionState | undefined,
    setActionHandler: vi.fn(
      (
        action: MediaSessionAction,
        handler: MediaSessionActionHandler | null,
      ) => {
        if (handler) handlers.set(action, handler);
        else handlers.delete(action);
      },
    ),
    setPositionState: vi.fn((position?: MediaPositionState) => {
      session.position = position;
    }),
  };
  class FixtureMediaMetadata {
    title: string;
    artist: string;
    album: string;
    artwork: MediaImage[];

    constructor(init: MediaMetadataInit = {}) {
      this.title = init.title ?? "";
      this.artist = init.artist ?? "";
      this.album = init.album ?? "";
      this.artwork = init.artwork ?? [];
    }
  }
  vi.stubGlobal("MediaMetadata", FixtureMediaMetadata);
  Object.defineProperty(navigator, "mediaSession", {
    configurable: true,
    value: session,
  });
  return {
    session,
    handlers,
    invoke(
      action: MediaSessionAction,
      details: Partial<MediaSessionActionDetails> = {},
    ) {
      const handler = handlers.get(action);
      if (!handler) throw new Error(`No system handler for ${action}`);
      handler({ action, ...details });
    },
    close() {
      if (original) Object.defineProperty(navigator, "mediaSession", original);
      else Reflect.deleteProperty(navigator, "mediaSession");
    },
  };
}
