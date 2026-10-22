import { log } from "../shared/log";

export function probeSessionArtwork(
  account: string,
  recording: string,
  ready: (image: MediaImage) => void,
) {
  const image = new Image();
  const url = new URL(
    `/api/v1/recordings/${encodeURIComponent(recording)}/artwork`,
    location.href,
  );
  const started = performance.now();
  const fields = { account_id: account, recording_id: recording };
  let active = true;
  const timer = setTimeout(() => finish("timeout"), 10_000);

  function finish(reason: string) {
    if (!active) return;
    active = false;
    clearTimeout(timer);
    image.removeEventListener("load", loaded);
    image.removeEventListener("error", failed);
    image.removeAttribute("src");
    log.debug("playback.system_artwork_finished", {
      ...fields,
      reason,
      duration_ms: Math.round(performance.now() - started),
    });
  }

  function loaded() {
    if (!active) return;
    const width = image.naturalWidth;
    const height = image.naturalHeight;
    if (
      !Number.isSafeInteger(width) ||
      width <= 0 ||
      !Number.isSafeInteger(height) ||
      height <= 0
    ) {
      finish("unavailable");
      return;
    }
    finish("ready");
    ready({ src: url.href, sizes: `${width}x${height}` });
  }

  function failed() {
    finish("unavailable");
  }

  image.addEventListener("load", loaded);
  image.addEventListener("error", failed);
  log.debug("playback.system_artwork_started", fields);
  image.src = url.href;
  return () => finish("cancelled");
}
