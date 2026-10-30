import type { Schema } from "../../../shared/schema";
import { afterEach, expect, test, vi } from "vitest";
import { prepareChange, submitChange } from "../../../shared/sync";
import { sessionApi } from "../api/sessionApi";
import { Playback, type MediaPort } from "../../playback/playback";
import { notifySessionChanged } from "../api/session";

const crypto = globalThis.crypto;
const randomUUID = Object.getOwnPropertyDescriptor(crypto, "randomUUID");
const uuidV4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 10;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
  }
  load() {}
  removeAttribute() {
    this.src = "";
  }
}

afterEach(() => {
  if (randomUUID) Object.defineProperty(crypto, "randomUUID", randomUUID);
  else Reflect.deleteProperty(crypto, "randomUUID");
});

test("HTTP LAN sessions, playback and prepared mutations keep secure UUIDs without crypto.randomUUID", async () => {
  localStorage.clear();
  Object.defineProperty(crypto, "randomUUID", {
    configurable: true,
    value: undefined,
  });
  expect(crypto.randomUUID).toBeUndefined();
  expect(crypto.getRandomValues(new Uint8Array(16))).toHaveLength(16);
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  fetch.mockResolvedValueOnce(new Response(null, { status: 200 }));

  await sessionApi.signIn(false, "fixture", "owned-fixture-password");
  const signedInRevision = localStorage.getItem("hito-session-revision");
  notifySessionChanged();
  const nextRevision = localStorage.getItem("hito-session-revision");
  const action: Schema<"Action"> = {
    kind: "playlist_follow_resume",
    playlist_id: "9fc57926-55f4-47ca-81e0-e8ca89c889cd",
    expected_generation: 3,
    installation_revision: 5,
    service_revision: 2,
  };
  const batch = prepareChange(action);
  const nextBatch = prepareChange(action);
  expect(nextBatch.device_id).toBe(batch.device_id);
  expect(localStorage.getItem("hito-device")).toBe(batch.device_id);
  const prepared = JSON.stringify(batch);
  const receipt = { acknowledged: [batch.operations[0].id] };
  const lost = new TypeError("Owned LAN fixture lost its response");
  fetch
    .mockRejectedValueOnce(lost)
    .mockResolvedValueOnce(Response.json(receipt));

  await expect(submitChange("account", batch)).rejects.toBe(lost);
  await expect(submitChange("account", batch)).resolves.toEqual(receipt);
  expect(JSON.stringify(batch)).toBe(prepared);
  for (const [path, options] of fetch.mock.calls.slice(1)) {
    expect(path).toBe("/api/v1/sync");
    expect(options?.body).toBe(prepared);
  }

  const player = new Playback(new Media(), async () => [
    { id: "source", ready: true },
  ]);
  const track = {
    id: "recording",
    title: "Owned fixture",
    artist: "Owned fixture",
    duration_ms: 10000,
    available: true,
  };
  await player.playQueue([track, track]);
  const entries = (await player.queuePage(0, 2)).entries.map(({ id }) => id);
  await player.selectEntry(entries[1]);
  expect((await player.queuePage(0, 2)).entries.map(({ id }) => id)).toEqual(
    entries,
  );
  const ids = [
    signedInRevision,
    nextRevision,
    batch.device_id,
    batch.operations[0].id,
    nextBatch.operations[0].id,
    ...entries,
  ];
  for (const id of ids) expect(id).toMatch(uuidV4);
  expect(new Set(ids).size).toBe(ids.length);
  player.clear();
});
