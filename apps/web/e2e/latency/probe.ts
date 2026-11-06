export type ProbeState = {
  clicked: number | null;
  playing: number | null;
  timeupdate: number | null;
  audible: number | null;
  errored: number | null;
  resumedPlaying: number | null;
  resumed: number | null;
  source: string | null;
  position: number;
};

export type Probe = {
  arm(): void;
  state(): ProbeState;
  silence(): void;
};

declare global {
  interface Window {
    __latency: Probe;
  }
}

export function installProbe() {
  const blank = (): ProbeState => ({
    clicked: null,
    playing: null,
    timeupdate: null,
    audible: null,
    errored: null,
    resumedPlaying: null,
    resumed: null,
    source: null,
    position: 0,
  });
  let state = blank();
  let armed = false;
  let baseline = 0;
  let check: (() => boolean) | null = null;
  let deadline = 0;
  const now = () => performance.timeOrigin + performance.now();
  const channel = new MessageChannel();
  channel.port1.onmessage = () => {
    if (!check) return;
    if (check() || performance.now() > deadline) {
      check = null;
      return;
    }
    channel.port2.postMessage(0);
  };
  const watch = (test: () => boolean) => {
    check = test;
    deadline = performance.now() + 20_000;
    channel.port2.postMessage(0);
  };
  const player = (event: Event) => {
    const target = event.target;
    return target instanceof HTMLAudioElement &&
      !("hitoPreload" in target.dataset)
      ? target
      : null;
  };
  document.addEventListener(
    "click",
    () => {
      if (armed && state.clicked === null) state.clicked = now();
    },
    true,
  );
  document.addEventListener(
    "playing",
    (event) => {
      const audio = player(event);
      if (!audio || !armed) return;
      if (state.errored === null) {
        state.playing ??= now();
        watch(() => {
          if (audio.currentTime > 0 && !audio.paused) {
            state.audible = now();
            state.source = audio.currentSrc;
            return true;
          }
          return false;
        });
      } else {
        state.resumedPlaying ??= now();
        watch(() => {
          if (audio.currentTime > baseline + 0.05 && !audio.paused) {
            state.resumed = now();
            state.source = audio.currentSrc;
            return true;
          }
          return false;
        });
      }
    },
    true,
  );
  document.addEventListener(
    "timeupdate",
    (event) => {
      const audio = player(event);
      if (audio && armed && audio.currentTime > 0) state.timeupdate ??= now();
    },
    true,
  );
  document.addEventListener(
    "error",
    (event) => {
      const audio = player(event);
      if (!audio || !armed || state.audible === null) return;
      state.errored ??= now();
      baseline = audio.currentTime;
    },
    true,
  );
  window.__latency = {
    arm() {
      state = blank();
      armed = true;
      check = null;
    },
    state() {
      const audio = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      return { ...state, position: audio?.currentTime ?? 0 };
    },
    silence() {
      armed = false;
      check = null;
      for (const audio of document.querySelectorAll("audio")) audio.pause();
    },
  };
}
