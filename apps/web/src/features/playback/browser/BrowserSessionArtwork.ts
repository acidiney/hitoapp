import { log } from "../../../shared/log";
import type { ArtworkKind } from "../../artwork/api/artworkRetrieval";

export type SessionArtworkCandidate = {
  kind: Exclude<ArtworkKind, "playlist">;
  id: string;
};

function artworkUrl({ kind, id }: SessionArtworkCandidate) {
  const collection = kind === "release" ? "releases" : "recordings";
  return new URL(
    `/api/v1/${collection}/${encodeURIComponent(id)}/artwork`,
    location.href,
  );
}

export function probeSessionArtwork(
  account: string,
  recording: string,
  candidates: readonly SessionArtworkCandidate[],
  ready: (image: MediaImage) => void,
  missing: (candidate: SessionArtworkCandidate) => void,
) {
  const started = performance.now();
  const fields = { account_id: account, recording_id: recording };
  let active = true;
  let index = 0;
  let image: HTMLImageElement | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let url: URL;

  function detach() {
    clearTimeout(timer);
    if (!image) return;
    image.removeEventListener("load", loaded);
    image.removeEventListener("error", failed);
    image.removeAttribute("src");
    image = undefined;
  }

  function finish(reason: string) {
    if (!active) return;
    active = false;
    detach();
    log.debug("playback.system_artwork_finished", {
      ...fields,
      reason,
      duration_ms: Math.round(performance.now() - started),
    });
  }

  function next(failedCandidate?: SessionArtworkCandidate) {
    detach();
    if (failedCandidate) missing(failedCandidate);
    if (!active) return;
    const candidate = candidates[index++];
    if (!candidate) {
      finish("unavailable");
      return;
    }
    url = artworkUrl(candidate);
    image = new Image();
    image.addEventListener("load", loaded);
    image.addEventListener("error", failed);
    timer = setTimeout(() => finish("timeout"), 10_000);
    image.src = url.href;
  }

  function loaded() {
    if (!active || !image) return;
    const width = image.naturalWidth;
    const height = image.naturalHeight;
    if (
      !Number.isSafeInteger(width) ||
      width <= 0 ||
      !Number.isSafeInteger(height) ||
      height <= 0
    ) {
      next();
      return;
    }
    const src = url.href;
    finish("ready");
    ready({ src, sizes: `${width}x${height}` });
  }

  function failed() {
    if (active) next(candidates[index - 1]);
  }

  log.debug("playback.system_artwork_started", fields);
  next();
  return () => finish("cancelled");
}
