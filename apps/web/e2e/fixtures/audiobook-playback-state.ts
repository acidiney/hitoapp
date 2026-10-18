import { createHash, randomUUID } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import {
  artistReviewDownloadAbsence,
  artistReviewReader,
} from "./artist-review-state";
import {
  preferencesLibraryState,
  seedPreferencesLibrary,
} from "./metadata-preferences-state";

type Schema = components["schemas"];
type Saved = Awaited<ReturnType<typeof seedPreferencesLibrary>>;

export const audiobookPlaybackUsername = "playback-admin";

export function audiobookPlaybackPrivateValues(password: string) {
  return [
    password,
    audiobookPlaybackUsername,
    "/music/",
    "/workspace/fixtures/e2e-generated/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ];
}

export function audiobookPlaybackState(page: Page, account: () => string) {
  const rawRead = artistReviewReader(page);
  let requests = 0;
  const budget = (amount = 1) => {
    requests += amount;
    if (requests > 100)
      throw new Error(
        "The audiobook playback public state budget was exceeded",
      );
  };
  const read = async <T>(path: string): Promise<T> => {
    budget();
    return rawRead<T>(path);
  };
  async function post<T>(path: string, data: Schema["SyncBatch"]): Promise<T> {
    budget();
    if (Buffer.byteLength(JSON.stringify(data)) > 16_384)
      throw new Error("The audiobook preparation request exceeded its bound");
    const response = await page.request.post(path, {
      headers: {
        "x-hito-request": "1",
        "x-hito-account": account(),
        Origin: new URL(page.url()).origin,
      },
      data,
      timeout: 10_000,
    });
    try {
      const bytes = await response.body();
      if (response.status() !== 200 || bytes.length > 4096)
        throw new Error(
          "The audiobook preparation acknowledgement was invalid",
        );
      return JSON.parse(bytes.toString()) as T;
    } finally {
      await response.dispose();
    }
  }
  return {
    read,
    requests: () => requests,
    async prepare(user: Schema["User"]) {
      const saved = await seedPreferencesLibrary(page, read, user);
      budget();
      const middle = randomUUID();
      const marker = randomUUID();
      const label = `Private playback bookmark ${randomUUID()}`;
      const title = `Private playback second title ${randomUUID()}`;
      const artist = `Private playback second artist ${randomUUID()}`;
      saved.privateValues.push(label, title, artist);
      const actions = [
        {
          kind: "entry_move",
          playlist_id: saved.playlist,
          entry_id: saved.entries[1],
          position: 2,
        },
        {
          kind: "entry_insert",
          playlist_id: saved.playlist,
          entry_id: middle,
          recording_id: saved.ids[1],
          position: 1,
        },
        { kind: "recording_title_set", recording_id: saved.ids[1], title },
        { kind: "recording_artist_set", recording_id: saved.ids[1], artist },
        { kind: "like_set", recording_id: saved.ids[1], liked: true },
        {
          kind: "progress_set",
          recording_id: saved.ids[0],
          position_ms: 73_400,
          speed: 1.25,
        },
        {
          kind: "progress_set",
          recording_id: saved.ids[1],
          position_ms: 800,
          speed: 1.5,
        },
        { kind: "audiobook_create", playlist_id: saved.playlist },
        {
          kind: "audiobook_bookmark_create",
          audiobook_id: saved.playlist,
          bookmark_id: marker,
          entry_id: saved.entries[1],
          recording_id: saved.ids[0],
          position_ms: 35_125,
          label,
        },
      ] satisfies Schema["Action"][];
      const batch = {
        device_id: "owned-audiobook-playback-preparation",
        operations: actions.map((action) => ({ id: randomUUID(), action })),
      } satisfies Schema["SyncBatch"];
      expect(
        isDeepStrictEqual(
          await post<Schema["SyncReceipt"]>("/api/v1/sync", batch),
          { acknowledged: batch.operations.map((operation) => operation.id) },
        ),
      ).toBe(true);
      return {
        ...saved,
        entries: [saved.entries[0], middle, saved.entries[1]],
        marker,
        label,
      };
    },
    async snapshot(saved: Saved) {
      budget(1 + saved.ids.length);
      return {
        ...(await preferencesLibraryState(page, read, saved)),
        settings: await read<Schema["MetadataSettings"]>(
          "/api/v1/metadata/settings",
        ),
        playlists: await read<Schema["Playlist"][]>("/api/v1/playlists"),
        book: await read<Schema["AudiobookDetail"]>(
          `/api/v1/audiobooks/${saved.playlist}`,
        ),
      };
    },
    async absence(saved: Saved) {
      budget(saved.ids.length + 1);
      const recordings = await artistReviewDownloadAbsence(page, saved.ids);
      const response = await page.request.get(
        `/api/v1/playlists/${saved.playlist}/download`,
        { timeout: 10_000 },
      );
      try {
        const bytes = await response.body();
        if (
          response.status() !== 404 ||
          bytes.length > 4096 ||
          !isDeepStrictEqual(JSON.parse(bytes.toString()), {
            error: "Resource not found",
          })
        )
          throw new Error(
            "The audiobook acquired an unrequested persistent download",
          );
      } finally {
        await response.dispose();
      }
      return recordings;
    },
  };
}

type Snapshot = Awaited<
  ReturnType<ReturnType<typeof audiobookPlaybackState>["snapshot"]>
>;

function ownedAudiobookWav(bytes: Buffer, pcmHash: string, infoHash: string) {
  if (
    bytes.length < 44 ||
    bytes.length > 4_194_304 ||
    bytes.toString("ascii", 0, 4) !== "RIFF" ||
    bytes.toString("ascii", 8, 12) !== "WAVE" ||
    bytes.readUInt32LE(4) !== bytes.length - 8
  )
    return false;
  const chunks = new Map<string, Buffer>();
  for (let offset = 12; offset < bytes.length; ) {
    if (chunks.size >= 3 || offset + 8 > bytes.length) return false;
    const kind = bytes.toString("ascii", offset, offset + 4);
    const size = bytes.readUInt32LE(offset + 4);
    const end = offset + 8 + size;
    const next = end + (size % 2);
    if (
      !["fmt ", "data", "LIST"].includes(kind) ||
      chunks.has(kind) ||
      next > bytes.length
    )
      return false;
    chunks.set(kind, bytes.subarray(offset + 8, end));
    offset = next;
  }
  const format = chunks.get("fmt ");
  const pcm = chunks.get("data");
  const info = chunks.get("LIST");
  return (
    chunks.size === 3 &&
    chunks.keys().next().value === "fmt " &&
    format?.length === 16 &&
    format.readUInt16LE(0) === 1 &&
    format.readUInt16LE(2) === 1 &&
    format.readUInt32LE(4) === 16_000 &&
    format.readUInt32LE(8) === 32_000 &&
    format.readUInt16LE(12) === 2 &&
    format.readUInt16LE(14) === 16 &&
    pcm?.length === 90 * 32_000 &&
    createHash("sha256").update(pcm).digest("hex") === pcmHash &&
    info?.toString("ascii", 0, 4) === "INFO" &&
    createHash("sha256").update(info).digest("hex") === infoHash
  );
}

export async function audiobookPlaybackAudio(before: Snapshot) {
  const owned = new Map<string, Buffer>();
  for (const [frequency, pcmHash, infoHash] of [
    [
      440,
      "b4a78fd7ea581892bde414e0c12206931d4dae472dd49d6470a4d3d82bc54a34",
      "619871204e2be5d53ef166c9d79113d742da97ef4e2f094ad494af066384f9bc",
    ],
    [
      660,
      "9b34c78ce4cadb2451b152a9340bccc1e9797c30702587350bc0cec3ce0c7652",
      "99f98f16186ff99cb6227ac368752bb8b342e2ca1e5a008f602d626fa49f5263",
    ],
  ] as const) {
    const folder = frequency === 440 ? "Music" : "Archive";
    const path = `/workspace/fixtures/e2e-generated/${folder}/Fixture ${frequency} Hz.wav`;
    const metadata = await stat(path);
    if (!metadata.isFile() || metadata.size < 44 || metadata.size > 4_194_304)
      throw new Error("The owned audiobook WAV exceeded its bound");
    const bytes = await readFile(path);
    if (!ownedAudiobookWav(bytes, pcmHash, infoHash))
      throw new Error(
        "The audiobook requires the owned ninety-second PCM WAV fixtures",
      );
    owned.set(`Fixture ${frequency} Hz`, bytes);
  }
  return before.sources.map((sources, index) => {
    const bytes = owned.get(before.metadata[index].title.inherited);
    if (
      !bytes ||
      sources.length !== 1 ||
      sources[0].provider !== "local" ||
      !sources[0].ready ||
      sources[0].can_prepare
    )
      throw new Error(
        "The audiobook requires one ready owned local source per chapter recording",
      );
    return { path: `/api/v1/sources/${sources[0].id}/audio`, bytes };
  });
}

type MediaObservations = {
  seeks: number[];
  starts: number[];
  natural_endings: number;
  overflow: boolean;
};

export async function observeAudiobookMedia(page: Page) {
  await page.evaluate(() => {
    const evidence = {
      seeks: [] as number[],
      starts: [] as number[],
      natural_endings: 0,
      overflow: false,
    };
    const observe = (event: Event) => {
      const audio = event.target;
      if (
        !(audio instanceof HTMLAudioElement) ||
        !audio.matches("[data-hito-player]")
      )
        return;
      if (event.type === "ended") {
        if (
          event.isTrusted &&
          Math.abs(audio.duration - 90) < 0.1 &&
          audio.currentTime >= audio.duration - 0.05
        )
          evidence.natural_endings++;
        else evidence.overflow = true;
        return;
      }
      const values =
        event.type === "seeking" ? evidence.seeks : evidence.starts;
      if (values.length >= 32) evidence.overflow = true;
      else values.push(Math.round(audio.currentTime * 1000));
    };
    document.addEventListener("seeking", observe, true);
    document.addEventListener("playing", observe, true);
    document.addEventListener("ended", observe, true);
    (
      globalThis as unknown as Record<string, unknown>
    ).__hitoOwnedAudiobookMedia = evidence;
  });
  return () =>
    page.evaluate(() => {
      const observed = (globalThis as unknown as Record<string, unknown>)
        .__hitoOwnedAudiobookMedia as MediaObservations;
      return {
        ...observed,
        seeks: observed.seeks.slice(),
        starts: observed.starts.slice(),
      };
    });
}

export async function audiobookMediaState(page: Page, path: string) {
  return page
    .locator("audio[data-hito-player]")
    .evaluate((element, expected) => {
      const audio = element as HTMLAudioElement;
      return {
        exact: audio.getAttribute("src") === expected,
        playing: !audio.paused && !audio.ended && audio.error === null,
        position: audio.currentTime,
        duration: audio.duration,
      };
    }, path);
}

export async function audiobookNavigationCheck(
  page: Page,
  path: string,
  documentOrigin: number,
  position: number,
) {
  const checkpoint = await page.evaluateHandle(
    ({ path, document: expectedDocument, position: expectedPosition }) => {
      const audio = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      if (!audio) throw new Error("The retained playback element was absent");
      const events: string[] = [];
      let overflow = false;
      let changed = false;
      const observed = [
        "pause",
        "play",
        "playing",
        "loadstart",
        "emptied",
        "abort",
        "ended",
        "error",
        "seeking",
        "seeked",
      ];
      const record = (event: Event) => {
        if (events.length >= 32) overflow = true;
        else events.push(event.type);
      };
      observed.forEach((event) => audio.addEventListener(event, record));
      const inspectMutations = (records: MutationRecord[]) => {
        if (
          records.some(
            (record) =>
              (record.type === "attributes" && record.target === audio) ||
              [...record.removedNodes].some(
                (node) => node === audio || node.contains(audio),
              ),
          )
        )
          changed = true;
      };
      const mutations = new MutationObserver(inspectMutations);
      mutations.observe(document.body, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ["src", "data-hito-player"],
      });
      return () => {
        inspectMutations(mutations.takeRecords());
        mutations.disconnect();
        observed.forEach((event) => audio.removeEventListener(event, record));
        const selected = document.querySelectorAll("audio[data-hito-player]");
        return (
          !changed &&
          !overflow &&
          events.length === 0 &&
          performance.timeOrigin === expectedDocument &&
          selected.length === 1 &&
          selected[0] === audio &&
          audio.isConnected &&
          audio.getAttribute("src") === path &&
          !audio.paused &&
          !audio.ended &&
          audio.error === null &&
          Number.isFinite(audio.currentTime) &&
          audio.currentTime > expectedPosition
        );
      };
    },
    { path, document: documentOrigin, position },
  );
  return async () => {
    try {
      return await checkpoint.evaluate((check) => check());
    } catch {
      return false;
    } finally {
      await checkpoint.dispose().catch(() => {});
    }
  };
}

export function preservedAudiobookPlayback(
  before: Snapshot,
  after: Snapshot,
  accepted: readonly Schema["SyncBatch"][],
  diagnose?: (reason: number, detail: number[]) => void,
) {
  const fail = (reason: number, detail: number[] = []) => {
    diagnose?.(reason, detail);
    return false;
  };
  const {
    progress: oldProgress,
    activity: oldActivity,
    events: oldEvents,
    ...oldState
  } = before;
  const { progress, activity, events, ...state } = after;
  if (!isDeepStrictEqual(state, oldState)) {
    const keys = [
      "libraries",
      "catalog",
      "artists",
      "releases",
      "playlist",
      "metadata",
      "sources",
      "choices",
      "settings",
      "playlists",
      "book",
    ] as const;
    return fail(1, [
      keys.reduce(
        (mask, key, index) =>
          mask |
          (Number(!isDeepStrictEqual(state[key], oldState[key])) << index),
        0,
      ),
    ]);
  }
  if (oldEvents.has_more || events.has_more)
    return fail(2, [Number(oldEvents.has_more), Number(events.has_more)]);
  if (
    !isDeepStrictEqual(
      events.operations.slice(0, oldEvents.operations.length),
      oldEvents.operations,
    )
  )
    return fail(3);
  const appended = events.operations.slice(oldEvents.operations.length);
  const expected = new Map(
    accepted.map((batch) => [batch.operations[0].id, batch]),
  );
  if (!appended.length || appended.length !== expected.size)
    return fail(4, [appended.length, expected.size]);
  const latest = new Map<string, Schema["RecordingProgress"]>();
  for (const event of appended) {
    if (!("operation" in event)) return fail(5);
    const batch = expected.get(event.operation.id);
    const action = event.operation.action;
    if (
      !batch ||
      batch.device_id !== event.device_id ||
      !isDeepStrictEqual(batch.operations[0], event.operation) ||
      action.kind !== "progress_set"
    )
      return fail(6, [
        Number(!!batch),
        Number(batch?.device_id === event.device_id),
        Number(isDeepStrictEqual(batch?.operations[0], event.operation)),
        Number(action.kind === "progress_set"),
      ]);
    latest.set(action.recording_id, {
      recording_id: action.recording_id,
      position_ms: action.position_ms,
      speed: action.speed,
    });
    expected.delete(event.operation.id);
  }
  if (
    expected.size ||
    latest.size !== 2 ||
    progress.length !== oldProgress.length ||
    !progress.every((item) =>
      isDeepStrictEqual(item, latest.get(item.recording_id)),
    )
  ) {
    const position = (value: number | undefined) =>
      typeof value === "number" &&
      Number.isSafeInteger(value) &&
      value >= 0 &&
      value <= 90_000
        ? value
        : -1;
    return fail(7, [
      expected.size,
      latest.size,
      progress.length,
      oldProgress.length,
      progress.reduce(
        (mask, item, index) =>
          mask |
          (Number(!isDeepStrictEqual(item, latest.get(item.recording_id))) <<
            Math.min(index, 30)),
        0,
      ),
      ...oldProgress
        .slice(0, 2)
        .flatMap((item) => [
          position(
            progress.find(
              (current) => current.recording_id === item.recording_id,
            )?.position_ms,
          ),
          position(latest.get(item.recording_id)?.position_ms),
        ]),
    ]);
  }
  const oldRows = new Map(
    oldActivity.history.map(
      ({
        position_ms: _position,
        speed: _speed,
        updated_at: _updated,
        ...row
      }) => [row.id, row],
    ),
  );
  if (activity.history.length !== oldActivity.history.length)
    return fail(8, [activity.history.length, oldActivity.history.length]);
  return activity.history.every(
    ({ position_ms, speed, updated_at, ...row }, index) => {
      const saved = latest.get(row.id);
      const prior = oldActivity.history.find((item) => item.id === row.id);
      const valid =
        saved &&
        prior &&
        updated_at >= prior.updated_at &&
        isDeepStrictEqual(row, oldRows.get(row.id)) &&
        position_ms === saved.position_ms &&
        speed === saved.speed;
      if (!valid)
        return fail(9, [
          index,
          Number(!!saved),
          Number(!!prior),
          Number(!!prior && updated_at >= prior.updated_at),
          Number(isDeepStrictEqual(row, oldRows.get(row.id))),
          Number(position_ms === saved?.position_ms),
          Number(speed === saved?.speed),
        ]);
      return valid;
    },
  );
}
