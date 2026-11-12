let origin: DOMRect | null = null;

export function rememberNowPlayingOrigin() {
  const cover = document.querySelector(".dock-artwork");
  origin = cover ? cover.getBoundingClientRect() : null;
}

export function nowPlayingOrigin(): DOMRect | null {
  return origin;
}
