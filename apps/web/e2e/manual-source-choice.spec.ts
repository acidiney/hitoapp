import { randomUUID } from "node:crypto";
import {
  expect,
  test,
  type BrowserContext,
  type Locator,
  type Page,
} from "@playwright/test";
import { manualSourceChoiceFixture } from "./fixtures/manual-source-choice";
import { captureConsolePrivacy } from "./fixtures/console-privacy";
import type { Schema } from "../src/shared/schema";

type Fixture = Awaited<ReturnType<typeof manualSourceChoiceFixture>>;
type Phase =
  | "configuration"
  | "setup"
  | "providers"
  | "import"
  | "personal_state"
  | "automatic_play"
  | "download_a"
  | "attach_b"
  | "attached_source_controls"
  | "choose_b"
  | "download_b"
  | "saved_b_playback"
  | "reset"
  | "isolation"
  | "retained_state"
  | "privacy"
  | "cleanup";
const optedIn = process.env.HITO_AUTOMATIC_SOURCE_E2E === "1";
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

test.use({ trace: "off", screenshot: "off", video: "off" });
test.skip(
  !optedIn,
  "Requires an opted-in fresh disposable manual source server",
);

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

async function clickResponse<T>(
  page: Page,
  control: Locator,
  path: string,
  status = 200,
): Promise<T> {
  const pending = page
    .waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().method() === "POST",
    )
    .catch(() => null);
  let clicked = false;
  try {
    await control.click();
    clicked = true;
  } catch {}
  const response = await pending;
  if (!clicked || response === null || response.status() !== status)
    throw new Error(
      "The explicit browser action must receive its accepted response",
    );
  return response.json();
}

async function collection(page: Page, recording: string, playlist: string) {
  const [metadata, detail, activity] = await Promise.all([
    read<Schema<"RecordingMetadata">>(
      page,
      `/api/v1/recordings/${recording}/metadata`,
    ),
    read<Schema<"PlaylistDetail">>(page, `/api/v1/playlists/${playlist}`),
    read<Schema<"Activity">>(page, "/api/v1/activity"),
  ]);
  return { metadata, detail, activity };
}
type Collection = Awaited<ReturnType<typeof collection>>;
function identity(snapshot: Collection) {
  return {
    metadata: {
      recording_id: snapshot.metadata.recording_id,
      title: snapshot.metadata.title,
      artist: snapshot.metadata.artist,
      selected: snapshot.metadata.selected_observation,
    },
    detail: {
      ...snapshot.detail,
      entries: snapshot.detail.entries.map((entry) => ({
        ...entry,
        available: false,
      })),
    },
    activity: {
      history: snapshot.activity.history.map((entry) => ({
        ...entry,
        available: false,
      })),
    },
  };
}
async function exactSavedBytes(page: Page, source: string, expected: Buffer) {
  const response = await page.request.get(`/api/v1/sources/${source}/audio`);
  expect(
    response.status() === 200 &&
      response.headers()["content-type"]?.startsWith("audio/wav"),
  ).toBe(true);
  expect((await response.body()).equals(expected)).toBe(true);
}
async function pause(page: Page) {
  await clickResponse<Schema<"SyncReceipt">>(
    page,
    page
      .getByRole("contentinfo", { name: "Audio player", exact: true })
      .getByRole("button", { name: "Pause", exact: true }),
    "/api/v1/sync",
  );
  expect(
    await page
      .locator("audio[data-hito-player]")
      .evaluate(
        (element: HTMLAudioElement) => element.paused && element.error === null,
      ),
  ).toBe(true);
}

test("manual source choice saves distinct B audio, plays it locally through provider outage and resets without losing private state", async ({
  page,
  context,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  let phase: Phase = "configuration";
  let failure: Phase | null = null;
  let provider: Fixture | null = null;
  const contexts: BrowserContext[] = [context];
  const callbacks = new Set<Promise<void>>();
  const issues = {
    page: 0,
    external: 0,
    privateLog: 0,
    callback: 0,
    consoleCallback: 0,
    requestCallback: 0,
    responseCallback: 0,
    consoleText: 0,
    consoleSerialization: 0,
    consoleBinding: 0,
    cleanup: 0,
    invalidAudio: 0,
  };
  const plainProbe = "owned-console-plain-canary";
  const nestedProbe = "owned-console-nested-canary";
  const consoleProbes = { text: 0, plain: 0, nested: 0 };
  const writes = {
    downloads: [] as Schema<"DownloadRequest">[],
    choices: [] as Schema<"ChooseSource">[],
    resets: [] as Schema<"ResetSourceChoice">[],
    reviews: 0,
    attaches: 0,
    resolutions: 0,
    adoptions: 0,
    preparations: 0,
  };
  const audioReads: { source: string; method: string; status: number }[] = [];
  const personalTitle = "Owned private listening label";
  const personalArtist = "Owned private music grouping";
  const draftValue = "Owned unsent source-choice draft";
  const owner = {
    username: "manual-owner",
    password: "owned browser owner password",
  } satisfies Schema<"Credentials">;
  const listener = {
    username: "manual-listener",
    password: "owned browser listener password",
  } satisfies Schema<"Credentials">;
  const privateValues = [
    personalTitle,
    personalArtist,
    draftValue,
    owner.username,
    owner.password,
    listener.username,
    listener.password,
  ];
  let origin = "";

  async function observe(owned: BrowserContext) {
    owned.on("page", (opened) => opened.on("pageerror", () => issues.page++));
    owned
      .pages()
      .forEach((opened) => opened.on("pageerror", () => issues.page++));
    await captureConsolePrivacy(
      owned,
      (text, capture) => {
        if (privateValues.some((value) => text.includes(value)))
          issues.privateLog++;
        if (text.includes(plainProbe))
          if (capture === "text") consoleProbes.text++;
          else consoleProbes.plain++;
        if (capture === "snapshot" && text.includes(nestedProbe))
          consoleProbes.nested++;
      },
      (failure) => {
        issues.callback++;
        issues.consoleCallback++;
        if (failure === "text") issues.consoleText++;
        else if (failure === "serialization") issues.consoleSerialization++;
        else issues.consoleBinding++;
      },
      callbacks,
    );
    owned.on("request", (request) => {
      try {
        const target = new URL(request.url());
        if (target.protocol.startsWith("http") && target.origin !== origin)
          issues.external++;
        if (target.origin !== origin || request.method() !== "POST") return;
        if (/\/source-choice$/.test(target.pathname))
          writes.choices.push(request.postDataJSON());
        else if (/\/source-choice\/reset$/.test(target.pathname))
          writes.resets.push(request.postDataJSON());
        else if (/\/download$/.test(target.pathname))
          writes.downloads.push(request.postDataJSON());
        else if (/\/sources\/review$/.test(target.pathname)) writes.reviews++;
        else if (/\/sources\/attach$/.test(target.pathname)) writes.attaches++;
        else if (/\/sources\/resolve$/.test(target.pathname))
          writes.resolutions++;
        else if (/\/plugins\/[^/]+\/recordings$/.test(target.pathname))
          writes.adoptions++;
        else if (/\/preparations?$/.test(target.pathname))
          writes.preparations++;
      } catch {
        issues.callback++;
        issues.requestCallback++;
      }
    });
    owned.on("response", (response) => {
      try {
        const target = new URL(response.url());
        const match = /^\/api\/v1\/sources\/([a-f0-9-]+)\/audio$/.exec(
          target.pathname,
        );
        if (target.origin !== origin || !match) return;
        audioReads.push({
          source: match[1],
          method: response.request().method(),
          status: response.status(),
        });
        if (
          ![200, 206].includes(response.status()) ||
          !response.headers()["content-type"]?.startsWith("audio/wav")
        )
          issues.invalidAudio++;
      } catch {
        issues.callback++;
        issues.responseCallback++;
      }
    });
  }
  async function settleCallbacks() {
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        (async () => {
          while (callbacks.size) await Promise.allSettled([...callbacks]);
        })(),
        new Promise<never>((_resolve, reject) => {
          timeout = setTimeout(
            () => reject(new Error("Owned browser callbacks timed out")),
            5000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timeout);
    }
  }

  try {
    if (!baseURL || process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error("The isolated browser privacy configuration is required");
    const configured = new URL(baseURL);
    if (
      configured.protocol !== "http:" ||
      !["localhost", "127.0.0.1", "e2e-server"].includes(configured.hostname) ||
      configured.port !== "8080" ||
      configured.pathname !== "/" ||
      configured.username ||
      configured.password ||
      configured.search ||
      configured.hash
    )
      throw new Error("The browser server must be isolated locally");
    origin = configured.origin;
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(10_000);
    await observe(context);
    const consoleProbe = await context.newPage();
    try {
      await consoleProbe.goto("about:blank");
      await consoleProbe.evaluate(
        ({ plain, nested }) => {
          window.addEventListener(
            "pagehide",
            () => {
              console.log(plain);
              console.info({ nested: { value: nested } });
            },
            { once: true },
          );
        },
        { plain: plainProbe, nested: nestedProbe },
      );
      await consoleProbe.goto("data:text/html,owned-console-probe");
      await expect
        .poll(() => consoleProbes)
        .toEqual({
          text: 1,
          plain: 1,
          nested: 1,
        });
      await settleCallbacks();
    } finally {
      await consoleProbe.close();
    }
    await context.addInitScript(() =>
      localStorage.setItem("hito-log-level", "debug"),
    );

    phase = "setup";
    await page.goto("/library");
    const setup = page.getByRole("button", {
      name: "Create administrator",
      exact: true,
    });
    await setup.waitFor({ state: "visible" });
    if (!(await setup.isVisible()))
      throw new Error("An uninitialized disposable server is required");
    await page
      .getByRole("textbox", { name: "Username", exact: true })
      .fill(owner.username);
    await page.getByLabel("Password", { exact: true }).fill(owner.password);
    await setup.click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    phase = "providers";
    const fixture = await manualSourceChoiceFixture();
    provider = fixture;
    privateValues.push(...fixture.privateValues);
    expect(fixture.audio("a").equals(fixture.audio("b"))).toBe(false);
    const headers = { "x-hito-request": "1", origin };
    const connections: Schema<"PluginInstallation">[] = [];
    for (const role of ["metadata", "a", "b"] as const) {
      const offered = await page.request.post("/api/v1/plugins/services", {
        headers,
        data: {
          name: `Owned ${role} service`,
          base_url: fixture.addresses[role],
        } satisfies Schema<"OfferPluginService">,
      });
      expect(offered.status()).toBe(201);
      const service: Schema<"PluginService"> = await offered.json();
      const installed = await page.request.post("/api/v1/plugins", {
        headers,
        data: {
          service_id: service.id,
          service_revision: service.revision,
          bearer_token: fixture.token,
        } satisfies Schema<"InstallPlugin">,
      });
      expect(installed.status()).toBe(201);
      connections.push(await installed.json());
    }

    phase = "import";
    await page.goto("/playlists");
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(connections[0].id);
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(fixture.reference);
    const imported = await clickResponse<Schema<"PlaylistImportReceipt">>(
      page,
      form.getByRole("button", { name: "Import a copy", exact: true }),
      `/api/v1/plugins/${connections[0].id}/playlists/import`,
    );
    await expect
      .poll(
        () =>
          new URL(page.url()).pathname === `/playlists/${imported.playlist_id}`,
      )
      .toBe(true);
    const initial = await read<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${imported.playlist_id}`,
    );
    const recording = initial.entries[0]?.recording_id;
    if (!recording)
      throw new Error(
        "The imported metadata must have a saved recording identity",
      );
    expect(
      uuid.test(recording) &&
        initial.entries.length === 2 &&
        initial.entries.every(
          (entry) => entry.recording_id === recording && !entry.available,
        ) &&
        new Set(initial.entries.map((entry) => entry.entry_id)).size === 2,
    ).toBe(true);
    const sourcesPath = `/api/v1/recordings/${recording}/sources`;
    const choicePath = `/api/v1/recordings/${recording}/source-choice`;
    const downloadPath = `/api/v1/recordings/${recording}/download`;
    expect((await read<Schema<"Source">[]>(page, sourcesPath)).length).toBe(0);
    expect((await page.request.get(downloadPath)).status()).toBe(404);
    expect(
      writes.downloads.length +
        audioReads.length +
        fixture.counts.a.audioGet +
        fixture.counts.b.audioGet,
    ).toBe(0);

    phase = "personal_state";
    const operations: Schema<"SyncBatch"> = {
      device_id: "manual-source-browser",
      operations: [
        {
          id: randomUUID(),
          action: {
            kind: "recording_title_set",
            recording_id: recording,
            title: personalTitle,
          },
        },
        {
          id: randomUUID(),
          action: {
            kind: "recording_artist_set",
            recording_id: recording,
            artist: personalArtist,
          },
        },
        {
          id: randomUUID(),
          action: { kind: "like_set", recording_id: recording, liked: true },
        },
        {
          id: randomUUID(),
          action: {
            kind: "progress_set",
            recording_id: recording,
            position_ms: 431,
            speed: 1.25,
          },
        },
      ],
    };
    const edited = await page.request.post("/api/v1/sync", {
      headers,
      data: operations,
    });
    expect(edited.status()).toBe(200);
    const personal = await collection(page, recording, imported.playlist_id);
    expect(
      personal.metadata.title.personal === personalTitle &&
        personal.metadata.artist.personal === personalArtist &&
        personal.metadata.selected_observation?.provider_recording_id ===
          fixture.original.id &&
        personal.detail.entries.every((entry) => entry.liked) &&
        personal.activity.history.some(
          (item) =>
            item.id === recording &&
            item.position_ms === 431 &&
            item.speed === 1.25 &&
            item.liked,
        ),
    ).toBe(true);

    phase = "automatic_play";
    await page.reload();
    await page
      .getByRole("button", { name: "Play playlist", exact: true })
      .click();
    const playing = page.locator("audio[data-hito-player]");
    await expect
      .poll(
        () =>
          playing.evaluate(
            (element: HTMLAudioElement) =>
              element.currentTime > 0.3 &&
              !element.paused &&
              element.error === null,
          ),
        { timeout: 20_000 },
      )
      .toBe(true);
    const autoSources = await read<Schema<"Source">[]>(page, sourcesPath);
    const sourceA = autoSources.find(
      (source) =>
        source.provider === "plugin" && source.label === fixture.labels.a,
    );
    if (!sourceA)
      throw new Error("Automatic playback must publish the verified A source");
    expect(
      await playing.evaluate(
        (element: HTMLAudioElement, path) =>
          new URL(element.src).pathname === path,
        `/api/v1/sources/${sourceA.id}/audio`,
      ),
    ).toBe(true);
    expect(
      fixture.counts.a.metadata > 0 &&
        fixture.counts.a.audioGet > 0 &&
        fixture.counts.b.audioGet === 0 &&
        fixture.canonicalAutomaticSearches([personalTitle, personalArtist]),
    ).toBe(true);
    expect(
      writes.downloads.length +
        writes.choices.length +
        writes.reviews +
        writes.attaches +
        writes.adoptions,
    ).toBe(0);
    expect((await page.request.get(downloadPath)).status()).toBe(404);
    await pause(page);
    const before = await collection(page, recording, imported.playlist_id);
    expect(
      JSON.stringify(identity(before).detail) ===
        JSON.stringify(identity(personal).detail),
    ).toBe(true);
    await page.goto(`/recordings/${recording}`);
    const audio = page.locator("audio[data-hito-player]");
    await audio.evaluate((element: HTMLAudioElement) => {
      const observed = { plays: 0 };
      element.addEventListener("play", () => observed.plays++);
      (
        element as HTMLAudioElement & { manualChoiceEvents: { plays: number } }
      ).manualChoiceEvents = observed;
    });
    const plays = () =>
      audio.evaluate(
        (element: HTMLAudioElement) =>
          (
            element as HTMLAudioElement & {
              manualChoiceEvents: { plays: number };
            }
          ).manualChoiceEvents.plays,
      );

    phase = "download_a";
    const download = page.getByRole("region", {
      name: "Download",
      exact: true,
    });
    const first = await clickResponse<Schema<"DownloadReceipt">>(
      page,
      download.getByRole("button", { name: "Download", exact: true }),
      downloadPath,
      202,
    );
    await expect(download.getByRole("status")).toHaveText(
      "Saved on this server",
      { timeout: 30_000 },
    );
    const savedA = await read<Schema<"DownloadStatus">>(page, downloadPath);
    if (!savedA.source_id)
      throw new Error("The explicit A download must publish its saved source");
    expect(
      savedA.status === "ready" &&
        savedA.intent_id === first.intent_id &&
        savedA.choice_revision === 0 &&
        savedA.matches_source_choice &&
        writes.downloads.length === 1 &&
        uuid.test(first.request_id),
    ).toBe(true);
    await exactSavedBytes(page, savedA.source_id, fixture.audio("a"));
    const availability = page.getByRole("region", {
      name: "Audio availability",
      exact: true,
    });
    await clickResponse<Schema<"Source">[]>(
      page,
      availability.getByRole("button", {
        name: "Check availability",
        exact: true,
      }),
      `${sourcesPath}/refresh`,
    );
    await expect(
      availability.getByRole("button", { name: "Play track", exact: true }),
    ).toBeEnabled();
    const draft = page.getByRole("textbox", {
      name: "Track title",
      exact: true,
    });
    await draft.fill(draftValue);
    const originalDraft = await draft.elementHandle();
    if (!originalDraft) throw new Error("The unsent metadata draft must exist");

    phase = "attach_b";
    fixture.showB();
    const attachment = page.getByRole("region", {
      name: "Add an audio source",
      exact: true,
    });
    await attachment
      .getByRole("button", { name: "Find a source", exact: true })
      .click();
    await attachment
      .getByRole("combobox", { name: "Music service", exact: true })
      .selectOption(connections[2].id);
    await attachment
      .getByRole("searchbox", { name: "Search music", exact: true })
      .fill(fixture.original.title);
    await attachment
      .getByRole("button", { name: "Search", exact: true })
      .click();
    await expect(
      attachment.getByRole("button", { name: "Review match", exact: true }),
    ).toBeEnabled();
    const review = await clickResponse<Schema<"ProviderSourceReview">>(
      page,
      attachment.getByRole("button", { name: "Review match", exact: true }),
      `${sourcesPath}/review`,
    );
    expect(review.candidate.id === fixture.recordings.b.id).toBe(true);
    const comparison = attachment.getByRole("dialog", {
      name: "Review source match",
      exact: true,
    });
    await expect(
      comparison.getByRole("button", { name: "Use this source", exact: true }),
    ).toBeEnabled();
    const attached = await clickResponse<Schema<"Recording">>(
      page,
      comparison.getByRole("button", { name: "Use this source", exact: true }),
      `${sourcesPath}/attach`,
    );
    expect(attached.id === recording).toBe(true);

    phase = "attached_source_controls";
    const choices = page.getByRole("region", {
      name: "Playback source",
      exact: true,
    });
    const bRadio = choices.getByRole("radio", {
      name: fixture.labels.b,
      exact: true,
    });
    await expect(bRadio).toBeEnabled();
    expect(
      await originalDraft.evaluate(
        (element, value) =>
          element.isConnected && (element as HTMLInputElement).value === value,
        draftValue,
      ),
    ).toBe(true);
    const afterAttachment = await read<Schema<"SourceChoice">>(
      page,
      choicePath,
    );
    const entryB = afterAttachment.sources.find(
      (entry) =>
        entry.source.label === fixture.labels.b &&
        entry.source.provider === "plugin",
    );
    if (!entryB)
      throw new Error(
        "The reviewed B source must have its existing Hito source identity",
      );
    expect(
      afterAttachment.revision === 0 && entryB.selectable && !entryB.selected,
    ).toBe(true);
    await expect.poll(fixture.activeCallbacks).toBe(0);
    const beforeChoice = fixture.snapshot();
    const beforeChoiceAudio = audioReads.length;
    const beforeChoiceResolutions = writes.resolutions;
    expect(await plays()).toBe(0);

    phase = "choose_b";
    await bRadio.check();
    const chosen = await clickResponse<Schema<"SourceChoiceReceipt">>(
      page,
      choices.getByRole("button", { name: "Use source", exact: true }),
      choicePath,
    );
    expect(
      chosen.recording_id === recording &&
        chosen.source_id === entryB.source.id &&
        chosen.revision === 1 &&
        uuid.test(chosen.request_id),
    ).toBe(true);
    await expect(bRadio).toBeChecked();
    await expect(download.getByRole("status")).toHaveText(
      "A copy from a previous source is saved on this server.",
    );
    await expect(
      download.getByRole("button", {
        name: "Download selected source",
        exact: true,
      }),
    ).toBeEnabled();
    const mismatch = await read<Schema<"DownloadStatus">>(page, downloadPath);
    expect(
      mismatch.intent_id === savedA.intent_id &&
        mismatch.source_id === savedA.source_id &&
        mismatch.status === "ready" &&
        mismatch.choice_revision === 1 &&
        !mismatch.matches_source_choice,
    ).toBe(true);
    expect(
      (
        await page.request.get(`/api/v1/sources/${savedA.source_id}/audio`)
      ).status(),
    ).toBe(404);
    expect(
      (await page.request.get(`/api/v1/sources/${sourceA.id}/audio`)).status(),
    ).toBe(404);
    expect(
      fixture.snapshot() === beforeChoice &&
        audioReads.length === beforeChoiceAudio &&
        writes.downloads.length === 1 &&
        writes.resolutions === beforeChoiceResolutions &&
        (await plays()) === 0,
    ).toBe(true);
    expect(
      JSON.stringify(
        identity(await collection(page, recording, imported.playlist_id)),
      ) === JSON.stringify(identity(before)),
    ).toBe(true);

    phase = "download_b";
    const second = await clickResponse<Schema<"DownloadReceipt">>(
      page,
      download.getByRole("button", {
        name: "Download selected source",
        exact: true,
      }),
      downloadPath,
      202,
    );
    await expect(download.getByRole("status")).toHaveText(
      "Saved on this server",
      { timeout: 30_000 },
    );
    const savedB = await read<Schema<"DownloadStatus">>(page, downloadPath);
    if (!savedB.source_id)
      throw new Error(
        "The explicit B download must publish its own saved source",
      );
    expect(
      savedB.status === "ready" &&
        savedB.intent_id === second.intent_id &&
        savedB.intent_id !== savedA.intent_id &&
        savedB.source_id !== savedA.source_id &&
        savedB.choice_revision === 1 &&
        savedB.matches_source_choice &&
        fixture.counts.b.audioGet > 0 &&
        writes.downloads.length === 2 &&
        first.request_id !== second.request_id &&
        uuid.test(second.request_id),
    ).toBe(true);
    await exactSavedBytes(page, savedB.source_id, fixture.audio("b"));
    expect(await plays()).toBe(0);
    expect(
      JSON.stringify(
        identity(await collection(page, recording, imported.playlist_id)),
      ) === JSON.stringify(identity(before)),
    ).toBe(true);

    phase = "saved_b_playback";
    fixture.setReady("b", false);
    fixture.setAudioUnavailable("b", true);
    await clickResponse<Schema<"Source">[]>(
      page,
      availability.getByRole("button", { name: "Check again", exact: true }),
      `${sourcesPath}/refresh`,
    );
    await expect(
      availability.getByRole("button", { name: "Play track", exact: true }),
    ).toBeEnabled();
    const unavailable = await read<Schema<"Source">[]>(page, sourcesPath);
    expect(
      unavailable.some(
        (source) => source.id === entryB.source.id && !source.ready,
      ) &&
        unavailable.some(
          (source) => source.id === savedB.source_id && source.ready,
        ),
    ).toBe(true);
    await expect.poll(fixture.activeCallbacks).toBe(0);
    const offlineCounts = fixture.snapshot();
    const beforeSavedReads = audioReads.length;
    await availability
      .getByRole("button", { name: "Play track", exact: true })
      .click();
    await expect
      .poll(
        () =>
          audio.evaluate(
            (element: HTMLAudioElement, path) =>
              !!element.getAttribute("src") &&
              new URL(element.src).pathname === path &&
              element.currentTime > 0.3 &&
              !element.paused &&
              element.error === null,
            `/api/v1/sources/${savedB.source_id}/audio`,
          ),
        { timeout: 20_000 },
      )
      .toBe(true);
    await pause(page);
    const dock = page.getByRole("contentinfo", {
      name: "Audio player",
      exact: true,
    });
    const seek = dock.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("Home");
    for (let step = 0; step < 50; step++) await seek.press("ArrowRight");
    await expect
      .poll(() =>
        audio.evaluate(
          (element: HTMLAudioElement) =>
            element.currentTime >= 4.9 &&
            element.currentTime <= 5.1 &&
            element.error === null,
        ),
      )
      .toBe(true);
    await dock.getByRole("button", { name: "Play", exact: true }).click();
    await expect
      .poll(() =>
        audio.evaluate(
          (element: HTMLAudioElement) =>
            element.currentTime > 5.3 &&
            !element.paused &&
            element.error === null,
        ),
      )
      .toBe(true);
    await pause(page);
    expect(
      audioReads
        .slice(beforeSavedReads)
        .some(
          (item) =>
            item.source === savedB.source_id &&
            item.method === "GET" &&
            [200, 206].includes(item.status),
        ) &&
        audioReads
          .slice(beforeSavedReads)
          .every((item) => item.source === savedB.source_id) &&
        fixture.snapshot() === offlineCounts,
    ).toBe(true);
    const afterPlayback = await collection(
      page,
      recording,
      imported.playlist_id,
    );
    expect(
      afterPlayback.activity.history.some(
        (item) =>
          item.id === recording && item.position_ms >= 5300 && item.liked,
      ),
    ).toBe(true);
    expect(
      JSON.stringify(identity(afterPlayback).metadata) ===
        JSON.stringify(identity(before).metadata) &&
        JSON.stringify(identity(afterPlayback).detail) ===
          JSON.stringify(identity(before).detail),
    ).toBe(true);

    phase = "reset";
    const latest = await read<Schema<"SourceChoice">>(page, choicePath);
    const mediaBeforeReset = await plays();
    const readsBeforeReset = audioReads.length;
    const resolutionsBeforeReset = writes.resolutions;
    const reset = await clickResponse<Schema<"SourceChoiceResetReceipt">>(
      page,
      choices.getByRole("button", { name: "Return to automatic", exact: true }),
      `${choicePath}/reset`,
    );
    expect(
      reset.recording_id === recording &&
        reset.revision === latest.revision + 1 &&
        uuid.test(reset.request_id) &&
        writes.resets.length === 1 &&
        writes.resets[0].expected_revision === latest.revision &&
        writes.resets[0].request_id === reset.request_id,
    ).toBe(true);
    const automatic = await read<Schema<"SourceChoice">>(page, choicePath);
    expect(
      automatic.mode === "automatic" &&
        automatic.revision === reset.revision &&
        automatic.sources.every((entry) => !entry.selected),
    ).toBe(true);
    await expect(
      choices.getByText("Automatic source selection", { exact: true }),
    ).toBeVisible();
    const radios = choices.getByRole("radio");
    expect(await radios.count()).toBeGreaterThan(0);
    for (const radio of await radios.all())
      expect(await radio.isChecked()).toBe(false);
    await expect(download.getByRole("status")).toHaveText(
      "Saved on this server",
    );
    expect(
      await originalDraft.evaluate(
        (element, value) =>
          element.isConnected && (element as HTMLInputElement).value === value,
        draftValue,
      ),
    ).toBe(true);
    expect(
      await audio.evaluate(
        (element: HTMLAudioElement) => element.paused && element.error === null,
      ),
    ).toBe(true);
    expect(
      (await plays()) === mediaBeforeReset &&
        audioReads.length === readsBeforeReset &&
        writes.resolutions === resolutionsBeforeReset &&
        writes.downloads.length === 2 &&
        fixture.snapshot() === offlineCounts,
    ).toBe(true);
    expect(
      JSON.stringify(
        identity(await collection(page, recording, imported.playlist_id)),
      ) === JSON.stringify(identity(afterPlayback)),
    ).toBe(true);

    phase = "isolation";
    const created = await page.request.post("/api/v1/users", {
      headers,
      data: listener,
    });
    expect(created.status()).toBe(201);
    const anonymous = await browser.newContext({
      baseURL,
      viewport: { width: 1440, height: 1000 },
    });
    contexts.push(anonymous);
    await observe(anonymous);
    const guest = await anonymous.newPage();
    await guest.goto(`/recordings/${recording}`);
    await expect(
      guest.getByRole("button", { name: "Sign in", exact: true }),
    ).toBeVisible();
    for (const path of [
      choicePath,
      downloadPath,
      `/api/v1/recordings/${recording}/metadata`,
      `/api/v1/playlists/${imported.playlist_id}`,
      `/api/v1/sources/${savedB.source_id}/audio`,
    ])
      expect((await guest.request.get(path)).status()).toBe(401);
    const bob = await browser.newContext({
      baseURL,
      viewport: { width: 1440, height: 1000 },
    });
    contexts.push(bob);
    await observe(bob);
    const other = await bob.newPage();
    await other.goto("/library");
    await expect(
      other.getByRole("button", { name: "Sign in", exact: true }),
    ).toBeVisible();
    await other
      .getByRole("textbox", { name: "Username", exact: true })
      .fill(listener.username);
    await other.getByLabel("Password", { exact: true }).fill(listener.password);
    await other.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      other.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    expect(
      (await read<Schema<"RecordingPage">>(other, "/api/v1/recordings")).items
        .length +
        (await read<Schema<"Playlist">[]>(other, "/api/v1/playlists")).length +
        (await read<Schema<"Activity">>(other, "/api/v1/activity")).history
          .length +
        (await read<Schema<"PluginSettings">>(other, "/api/v1/plugins"))
          .installations.length,
    ).toBe(0);
    for (const path of [
      choicePath,
      downloadPath,
      `/api/v1/recordings/${recording}/metadata`,
      `/api/v1/playlists/${imported.playlist_id}`,
      `/api/v1/sources/${savedB.source_id}/audio`,
    ])
      expect((await other.request.get(path)).status()).toBe(404);
    for (const [path, data] of [
      [
        choicePath,
        {
          request_id: randomUUID(),
          source_id: entryB.source.id,
        } satisfies Schema<"ChooseSource">,
      ],
      [
        `${choicePath}/reset`,
        {
          request_id: randomUUID(),
          expected_revision: reset.revision,
        } satisfies Schema<"ResetSourceChoice">,
      ],
    ] as const)
      expect((await other.request.post(path, { headers, data })).status()).toBe(
        404,
      );
    expect(
      (await other.getByText(personalTitle, { exact: true }).count()) +
        (await other.getByText(personalArtist, { exact: true }).count()),
    ).toBe(0);

    phase = "retained_state";
    const finalChoice = await read<Schema<"SourceChoice">>(page, choicePath);
    const finalDownload = await read<Schema<"DownloadStatus">>(
      page,
      downloadPath,
    );
    const finalState = await collection(page, recording, imported.playlist_id);
    expect(
      finalChoice.mode === "automatic" &&
        finalChoice.revision === reset.revision &&
        finalDownload.status === "ready" &&
        finalDownload.intent_id === savedB.intent_id &&
        finalDownload.source_id === savedB.source_id &&
        finalDownload.choice_revision === reset.revision &&
        finalDownload.matches_source_choice &&
        JSON.stringify(identity(finalState)) ===
          JSON.stringify(identity(afterPlayback)),
    ).toBe(true);
    expect(
      finalState.metadata.observations.items.some(
        (item) =>
          item.plugin_installation_id === connections[1].id &&
          item.provider_recording_id === fixture.recordings.a.id,
      ) &&
        finalState.metadata.observations.items.some(
          (item) =>
            item.plugin_installation_id === connections[2].id &&
            item.provider_recording_id === fixture.recordings.b.id,
        ) &&
        finalState.detail.entries.every(
          (entry) => entry.available && entry.liked,
        ),
    ).toBe(true);
    expect(
      fixture.snapshot() === offlineCounts &&
        writes.choices.length === 1 &&
        writes.choices[0].source_id === entryB.source.id &&
        writes.choices[0].request_id === chosen.request_id &&
        writes.downloads.length === 2 &&
        writes.downloads[0].request_id === first.request_id &&
        writes.downloads[1].request_id === second.request_id &&
        writes.reviews === 1 &&
        writes.attaches === 1 &&
        writes.adoptions + writes.preparations === 0,
    ).toBe(true);
    expect(
      (await plays()) === mediaBeforeReset &&
        audioReads.length === readsBeforeReset &&
        writes.resolutions === resolutionsBeforeReset &&
        (await audio.evaluate(
          (element: HTMLAudioElement) =>
            element.paused && element.error === null,
        )),
    ).toBe(true);
    phase = "privacy";
    await settleCallbacks();
    expect(
      issues.page +
        issues.external +
        issues.privateLog +
        issues.callback +
        issues.invalidAudio +
        fixture.counts.protocol +
        fixture.counts.unexpected +
        fixture.counts.handler,
    ).toBe(0);
  } catch {
    failure = phase;
  } finally {
    try {
      await settleCallbacks();
    } catch {
      issues.cleanup++;
    }
    for (const owned of contexts.reverse()) {
      try {
        await owned.close();
      } catch {
        issues.cleanup++;
      }
    }
    try {
      await settleCallbacks();
    } catch {
      issues.cleanup++;
    }
    try {
      await provider?.close();
    } catch {
      issues.cleanup++;
    }
    if (callbacks.size || provider?.activeCallbacks()) issues.cleanup++;
    if (issues.cleanup) failure ??= "cleanup";
    if (
      issues.page +
      issues.external +
      issues.privateLog +
      issues.callback +
      issues.invalidAudio +
      (provider?.counts.protocol ?? 0) +
      (provider?.counts.unexpected ?? 0) +
      (provider?.counts.handler ?? 0)
    )
      failure ??= "privacy";
  }
  if (failure !== null) {
    const error = new Error(
      [
        `Manual source browser check failed at phase ${failure}`,
        `page=${issues.page}`,
        `external=${issues.external}`,
        `privateLog=${issues.privateLog}`,
        `callback=${issues.callback}`,
        `consoleCallback=${issues.consoleCallback}`,
        `requestCallback=${issues.requestCallback}`,
        `responseCallback=${issues.responseCallback}`,
        `consoleText=${issues.consoleText}`,
        `consoleSerialization=${issues.consoleSerialization}`,
        `consoleBinding=${issues.consoleBinding}`,
        `cleanup=${issues.cleanup}`,
        `invalidAudio=${issues.invalidAudio}`,
        `protocol=${provider?.counts.protocol ?? 0}`,
        `unexpected=${provider?.counts.unexpected ?? 0}`,
        `handler=${provider?.counts.handler ?? 0}`,
        `callbackPending=${callbacks.size}`,
      ].join("; "),
    );
    error.stack = error.message;
    throw error;
  }
});
