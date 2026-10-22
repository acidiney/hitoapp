import { vi } from "vitest";

export const chapterAudioPath = (source: string) =>
  `/api/v1/sources/${source}/audio`;

export function audiobookContinuationMedia() {
  const created: HTMLAudioElement[] = [];
  const loaded: { media: HTMLAudioElement; source: string | null }[] = [];
  const played: { media: HTMLAudioElement; source: string | null }[] = [];
  const states = new WeakMap<
    HTMLAudioElement,
    { paused: boolean; ended: boolean; ready: number }
  >();

  vi.stubGlobal("Audio", function () {
    const media = document.createElement("audio");
    const state = { paused: true, ended: false, ready: 0 };
    states.set(media, state);
    created.push(media);
    Object.defineProperties(media, {
      paused: { get: () => state.paused },
      ended: { get: () => state.ended },
      duration: { get: () => 120 },
      readyState: { get: () => state.ready },
      seeking: { get: () => false },
      currentSrc: { get: () => media.src },
      play: {
        value: async () => {
          state.paused = false;
          state.ended = false;
          played.push({ media, source: media.getAttribute("src") });
          media.dispatchEvent(new Event("play"));
        },
      },
      pause: {
        value: () => {
          const changed = !state.paused;
          state.paused = true;
          if (changed) media.dispatchEvent(new Event("pause"));
        },
      },
      load: {
        value: () => {
          loaded.push({ media, source: media.getAttribute("src") });
          media.currentTime = 0;
          state.ended = false;
          state.ready = 0;
          media.dispatchEvent(new Event("loadstart"));
        },
      },
    });
    return media;
  });

  const ready = (media: HTMLAudioElement) => {
    states.get(media)!.ready = 1;
    media.dispatchEvent(new Event("loadedmetadata"));
    states.get(media)!.ready = 4;
    media.dispatchEvent(new Event("canplay"));
  };

  return {
    created,
    loaded,
    played,
    ready,
    current() {
      const media = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      if (!media) throw new Error("The account has no active audio element");
      return media;
    },
    prepared(source: string) {
      return created.find(
        (media) =>
          media.isConnected &&
          media.hasAttribute("data-hito-preload") &&
          media.getAttribute("src") === chapterAudioPath(source),
      );
    },
    observePlaying(media: HTMLAudioElement) {
      if (media.paused || media.readyState < 3 || media.currentTime !== 0)
        throw new Error("The owned source must be ready and playing at zero");
      media.dispatchEvent(new Event("playing"));
      media.dispatchEvent(new Event("timeupdate"));
    },
    end(media: HTMLAudioElement) {
      media.currentTime = media.duration;
      states.get(media)!.paused = true;
      states.get(media)!.ended = true;
      media.dispatchEvent(new Event("ended"));
    },
  };
}
