import { vi } from "vitest";

export function audiobookProgressMedia() {
  const created: HTMLAudioElement[] = [];
  const loaded: { media: HTMLAudioElement; source: string | null }[] = [];
  const played: { media: HTMLAudioElement; source: string | null }[] = [];
  const states = new WeakMap<
    HTMLAudioElement,
    {
      paused: boolean;
      seeking: boolean;
      ready: number;
      position: number;
    }
  >();
  vi.stubGlobal("Audio", function () {
    const media = document.createElement("audio");
    const state = { paused: true, seeking: false, ready: 0, position: 0 };
    states.set(media, state);
    created.push(media);
    Object.defineProperties(media, {
      paused: { get: () => state.paused },
      ended: { get: () => false },
      duration: { get: () => 120 },
      readyState: { get: () => state.ready },
      seeking: { get: () => state.seeking },
      currentSrc: { get: () => media.src },
      currentTime: {
        get: () => state.position,
        set: (position: number) => {
          if (position !== state.position) state.seeking = true;
          state.position = position;
        },
      },
      play: {
        value: async () => {
          state.paused = false;
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
          state.position = 0;
          state.ready = 0;
          state.seeking = false;
          media.dispatchEvent(new Event("loadstart"));
        },
      },
    });
    return media;
  });
  return {
    created,
    loaded,
    played,
    current() {
      const media = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      if (!media) throw new Error("The account has no active audio element");
      return media;
    },
    ready(media: HTMLAudioElement) {
      states.get(media)!.ready = 1;
      media.dispatchEvent(new Event("loadedmetadata"));
      states.get(media)!.ready = 4;
      media.dispatchEvent(new Event("canplay"));
    },
    observePlaying(media: HTMLAudioElement) {
      if (media.paused || media.readyState < 3 || media.currentTime !== 0)
        throw new Error("The owned source must be ready and playing at zero");
      media.dispatchEvent(new Event("playing"));
      media.dispatchEvent(new Event("timeupdate"));
    },
    completeSeek(media: HTMLAudioElement) {
      if (!media.seeking)
        throw new Error("The owned source has no pending seek");
      states.get(media)!.seeking = false;
      media.dispatchEvent(new Event("seeked"));
      media.dispatchEvent(new Event("timeupdate"));
    },
  };
}
