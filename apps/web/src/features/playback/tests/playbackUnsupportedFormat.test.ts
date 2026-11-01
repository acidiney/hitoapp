import { afterEach, expect, test, vi } from "vitest";
import { Playback, type MediaPort } from "../playback";

const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 10;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  error: { code: number } | null = null;
  rejection: Error | null = null;
  async play() {
    if (this.rejection) throw this.rejection;
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {}
  removeAttribute() {
    this.src = "";
  }
}

const track = (id: string) => ({
  id,
  title: id,
  artist: "Fixture",
  duration_ms: 10000,
  available: true,
});

const UNSUPPORTED = "This device can't play this audio format.";

afterEach(() => vi.unstubAllGlobals());

function probeReturns(status: number, type: string) {
  const fetcher = vi.fn(
    async () =>
      new Response("x", { status, headers: { "content-type": type } }),
  );
  vi.stubGlobal("fetch", fetcher);
  return fetcher;
}

async function playing() {
  const media = new Media();
  const player = new Playback(media, async (_id, options) =>
    options.purpose === "cached" ? [{ id: "first", ready: true }] : [],
  );
  await player.playQueue([track("first")]);
  return { media, player };
}

async function refuse(media: Media, code: number) {
  media.error = { code };
  media.dispatchEvent(new Event("error"));
}

test("a source that was delivered but refused by the element reports an unsupported format", async () => {
  const fetcher = probeReturns(206, "audio/flac");
  const { media, player } = await playing();
  await refuse(media, MEDIA_ERR_SRC_NOT_SUPPORTED);
  await vi.waitFor(() => expect(player.snapshot().error).toBe(UNSUPPORTED));
  expect(fetcher).toHaveBeenCalledWith(
    "/api/v1/sources/first/audio",
    expect.objectContaining({ headers: { Range: "bytes=0-0" } }),
  );
});

test("a missing or expired source keeps the generic unavailable message", async () => {
  probeReturns(404, "application/json");
  const { media, player } = await playing();
  await refuse(media, MEDIA_ERR_SRC_NOT_SUPPORTED);
  await vi.waitFor(() =>
    expect(player.snapshot().error).toBe("Audio is unavailable for first."),
  );
});

test("a probe that fails or returns another content type keeps the generic message", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => {
      throw new TypeError("network");
    }),
  );
  const failed = await playing();
  await refuse(failed.media, MEDIA_ERR_SRC_NOT_SUPPORTED);
  await vi.waitFor(() =>
    expect(failed.player.snapshot().error).toBe(
      "Audio is unavailable for first.",
    ),
  );
  probeReturns(200, "text/html");
  const html = await playing();
  await refuse(html.media, MEDIA_ERR_SRC_NOT_SUPPORTED);
  await vi.waitFor(() =>
    expect(html.player.snapshot().error).toBe("Audio is unavailable for first."),
  );
});

test("a play rejection for a delivered source reports an unsupported format", async () => {
  probeReturns(200, "application/octet-stream");
  const media = new Media();
  media.rejection = Object.assign(new Error("no"), {
    name: "NotSupportedError",
  });
  const player = new Playback(media, async (_id, options) =>
    options.purpose === "cached" ? [{ id: "first", ready: true }] : [],
  );
  await player.playQueue([track("first")]);
  await vi.waitFor(() => expect(player.snapshot().error).toBe(UNSUPPORTED));
});

test("other media failures keep the generic unavailable message", async () => {
  const fetcher = probeReturns(206, "audio/flac");
  const { media, player } = await playing();
  await refuse(media, MEDIA_ERR_NETWORK);
  await vi.waitFor(() =>
    expect(player.snapshot().error).toBe("Audio is unavailable for first."),
  );
  expect(fetcher).not.toHaveBeenCalled();
});

test("a late unsupported event while the next track resolves cannot label it", async () => {
  probeReturns(206, "audio/flac");
  const media = new Media();
  let resolve: ((sources: { id: string; ready: boolean }[]) => void) | null =
    null;
  const player = new Playback(media, (id) =>
    id === "first"
      ? Promise.resolve([{ id: "first", ready: true }])
      : resolve
        ? Promise.resolve([])
        : new Promise((done) => {
            resolve = done;
          }),
  );
  await player.playQueue([track("first")]);
  const opening = player.playQueue([track("second")]);
  await vi.waitFor(() => expect(resolve).not.toBeNull());
  await refuse(media, MEDIA_ERR_SRC_NOT_SUPPORTED);
  resolve!([]);
  await opening;
  await vi.waitFor(() =>
    expect(player.snapshot().error).toBe("Audio is unavailable for second."),
  );
});
