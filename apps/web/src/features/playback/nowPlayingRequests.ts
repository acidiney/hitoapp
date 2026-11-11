type Listener = () => void;

const listeners = new Set<Listener>();

export function requestNowPlaying() {
  listeners.forEach((listener) => listener());
}

export function onNowPlayingRequest(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
