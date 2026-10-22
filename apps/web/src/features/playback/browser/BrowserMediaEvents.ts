import type { MediaPort } from "../model";

export type MediaHandlers = {
  loadedMetadata(): void;
  durationChange(): void;
  timeUpdate(): void;
  play(): void;
  playing(): void;
  seeked(): void;
  seeking(): void;
  pause(): void;
  error(): void;
  ended(): void;
  canPlay(): void;
  interrupted(): void;
};

export function listenToMedia(
  media: MediaPort,
  isCurrent: () => boolean,
  handlers: MediaHandlers,
) {
  const listeners: [string, EventListener][] = [];
  const listen = (event: string, action: () => void) => {
    const listener = () => {
      if (isCurrent()) action();
    };
    media.addEventListener(event, listener);
    listeners.push([event, listener]);
  };
  listen("loadedmetadata", handlers.loadedMetadata);
  listen("durationchange", handlers.durationChange);
  listen("timeupdate", handlers.timeUpdate);
  listen("play", handlers.play);
  listen("playing", handlers.playing);
  listen("seeked", handlers.seeked);
  listen("pause", handlers.pause);
  listen("error", handlers.error);
  listen("ended", handlers.ended);
  listen("seeking", handlers.seeking);
  listen("canplay", handlers.canPlay);
  for (const event of ["waiting", "stalled", "ratechange", "loadstart"])
    listen(event, handlers.interrupted);
  return () => {
    for (const [event, listener] of listeners)
      media.removeEventListener(event, listener);
  };
}
