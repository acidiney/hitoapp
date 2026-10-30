import { randomUUID } from "node:crypto";
import {
  expect,
  test,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import { playlistFixture } from "./fixtures/playlist";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";

type CollectionState = {
  playlists: Schema<"Playlist">[];
  playlist: Schema<"PlaylistDetail">;
  activity: Schema<"Activity">;
  feed: Schema<"SyncPage">;
};

declare global {
  interface Window {
    captureAttachmentHistory: (url: string, state: string) => Promise<void>;
  }
}

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/library");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

async function get<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

async function collection(
  page: Page,
  playlist: string,
): Promise<CollectionState> {
  return {
    playlists: await get(page, "/api/v1/playlists"),
    playlist: await get(page, `/api/v1/playlists/${playlist}`),
    activity: await get(page, "/api/v1/activity"),
    feed: await get(page, "/api/v1/sync?cursor=0"),
  };
}

function retainedCollection(
  state: CollectionState,
  recording: string,
): CollectionState {
  return {
    ...state,
    playlist: {
      ...state.playlist,
      entries: state.playlist.entries.map((entry) =>
        entry.recording_id === recording
          ? { ...entry, available: false, plays_from_services: false }
          : entry,
      ),
    },
    activity: {
      history: state.activity.history.map((entry) =>
        entry.id === recording
          ? { ...entry, available: false, plays_from_services: false }
          : entry,
      ),
    },
  };
}

async function layouts(
  page: Page,
  info: TestInfo,
  stage: string,
  controls: Locator[],
) {
  for (const viewport of [
    { width: 1440, height: 1000 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    for (const control of controls) {
      await control.evaluate((element) =>
        element.scrollIntoView({ block: "center", inline: "center" }),
      );
      await expect(control).toBeVisible();
      const bounds = await control.boundingBox();
      const dock = await page
        .getByRole("contentinfo", { name: "Audio player" })
        .boundingBox();
      const label =
        (await control.getAttribute("aria-label")) ??
        (await control.innerText());
      expect(
        bounds &&
          dock &&
          bounds.x >= 0 &&
          bounds.x + bounds.width <= viewport.width &&
          bounds.y + bounds.height <= dock.y,
        `${stage} ${viewport.width}px ${label} bounds`,
      ).toBe(true);
      expect(
        await control.evaluate((element) => {
          const box = element.getBoundingClientRect();
          const hit = document.elementFromPoint(
            box.x + box.width / 2,
            box.y + box.height / 2,
          );
          return hit !== null && element.contains(hit);
        }),
      ).toBe(true);
    }
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: info.outputPath(`${stage}-${viewport.width}.png`),
    });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
}

test("a copied provider track attaches reviewed sources and recovers actual audio without changing its saved identity", async ({
  page,
  browser,
  baseURL,
}, info) => {
  if (!baseURL)
    throw new Error("The disposable e2e server URL must be configured");
  const playlist = await playlistFixture();
  const providers = [
    await providerFixture("Attachment A"),
    await providerFixture("Attachment B"),
  ];
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const services: Schema<"PluginService">[] = [];
  const connections: Schema<"PluginInstallation">[] = [];
  const errors: string[] = [];
  const consoleErrors: { text: string; url: string }[] = [];
  const failedResponses: {
    url: string;
    method: string;
    status: number;
    beforeSignIn: boolean;
    afterInjectedFailure: boolean;
  }[] = [];
  const logs: Promise<string>[] = [];
  const urls: string[] = [];
  const history: { url: string; state: string }[] = [];
  const attachments: Schema<"AttachReviewedProviderSource">[] = [];
  let adoptions = 0;
  let copied: string | null = null;
  let signedIn = false;
  let injectedFailure = false;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error")
      consoleErrors.push({ text: message.text(), url: message.location().url });
    logs.push(
      Promise.all(message.args().map((argument) => argument.jsonValue()))
        .then((values) => JSON.stringify(values))
        .catch(() => message.text()),
    );
  });
  page.on("response", (response) => {
    if (
      response.status() < 400 ||
      new URL(response.url()).origin !== new URL(baseURL).origin
    )
      return;
    failedResponses.push({
      url: response.url(),
      method: response.request().method(),
      status: response.status(),
      beforeSignIn: !signedIn,
      afterInjectedFailure: injectedFailure,
    });
  });
  page.on("request", (request) => {
    urls.push(request.url());
    if (request.method() !== "POST") return;
    const path = new URL(request.url()).pathname;
    if (path.endsWith("/sources/attach"))
      attachments.push(request.postDataJSON());
    if (/\/plugins\/[^/]+\/recordings$/.test(path)) adoptions++;
  });
  await page.exposeFunction(
    "captureAttachmentHistory",
    (url: string, state: string) => {
      history.push({ url, state });
    },
  );
  await page.addInitScript(() => {
    localStorage.setItem("hito-log-level", "debug");
    for (const method of ["pushState", "replaceState"] as const) {
      const original = window.history[method].bind(window.history);
      window.history[method] = (state, unused, url) => {
        void window.captureAttachmentHistory(
          String(url ?? location.href),
          JSON.stringify(state) ?? "null",
        );
        original(state, unused, url);
      };
    }
  });
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    signedIn = true;
    for (const [index, fixture] of [playlist, ...providers].entries()) {
      const offered = await page.request.post("/api/v1/plugins/services", {
        headers,
        data: {
          name: `${playlist.prefix} source ${index}`,
          base_url: fixture.address,
        } satisfies Schema<"OfferPluginService">,
      });
      expect(offered.status()).toBe(201);
      const service: Schema<"PluginService"> = await offered.json();
      services.push(service);
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
    const origin = connections[0];
    const candidates = connections.slice(1);
    await page.goto("/playlists");
    const form = page.getByRole("region", { name: "Import a playlist" });
    await form
      .getByRole("textbox", { name: "Playlist link or identifier" })
      .fill(playlist.reference);
    const imported = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/plugins/${origin.id}/playlists/import` &&
        response.request().method() === "POST",
    );
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    const importResponse = await imported;
    expect(importResponse.status()).toBe(200);
    const receipt: Schema<"PlaylistImportReceipt"> =
      await importResponse.json();
    copied = receipt.playlist_id;
    await expect(page).toHaveURL(new URL(`/playlists/${copied}`, baseURL).href);
    const importedDetail = await get<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${copied}`,
    );
    const recording = importedDetail.entries[0].recording_id;
    if (!recording)
      throw new Error("Copied known metadata needs a saved recording UUID");
    expect(importedDetail.entries).toHaveLength(4);
    expect(importedDetail.entries[3].recording_id).toBe(recording);
    expect(importedDetail.entries.map((entry) => entry.origin?.kind)).toEqual([
      "recording",
      "unavailable",
      "unknown",
      "recording",
    ]);
    expect(
      new Set(importedDetail.entries.map((entry) => entry.entry_id)).size,
    ).toBe(4);
    expect(await get(page, `/api/v1/recordings/${recording}/sources`)).toEqual(
      [],
    );
    const importedCatalog = await get<Schema<"RecordingPage">>(
      page,
      `/api/v1/recordings?q=${encodeURIComponent(playlist.prefix)}`,
    );
    const importedRecording = importedCatalog.items.find(
      (item) => item.id === recording,
    );
    if (!importedRecording)
      throw new Error(
        "The copied recording must belong to its personal catalog",
      );
    const catalogPath = `/api/v1/recordings?library=${importedRecording.library_id}`;
    const title = `${playlist.prefix} personal title`;
    const artist = `${playlist.prefix} personal artist`;
    const saved = await page.request.post("/api/v1/sync", {
      headers,
      data: {
        device_id: "attachment-e2e",
        operations: [
          {
            id: randomUUID(),
            action: {
              kind: "recording_title_set",
              recording_id: recording,
              title,
            },
          },
          {
            id: randomUUID(),
            action: {
              kind: "recording_artist_set",
              recording_id: recording,
              artist,
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
              position_ms: 4_200,
              speed: 1.25,
            },
          },
        ],
      } satisfies Schema<"SyncBatch">,
    });
    expect(saved.status()).toBe(200);
    const removed = await page.request.delete(`/api/v1/plugins/${origin.id}`, {
      headers,
      data: { revision: origin.revision },
    });
    expect(removed.status()).toBe(204);
    connections.shift();
    playlist.offline();
    const before = await collection(page, copied);
    const catalog = await get<Schema<"RecordingPage">>(page, catalogPath);
    const metadata = await get<Schema<"RecordingMetadata">>(
      page,
      `/api/v1/recordings/${recording}/metadata`,
    );
    const primary = catalog.items.find((item) => item.id === recording);
    if (!primary)
      throw new Error(
        "The edited recording must retain its personal catalog identity",
      );
    expect(primary).toMatchObject({
      title,
      artist,
      liked: true,
      available: false,
    });
    expect(metadata.title.personal).toBe(title);
    expect(metadata.artist.personal).toBe(artist);
    expect(
      before.activity.history.find((item) => item.id === recording),
    ).toMatchObject({ position_ms: 4_200, speed: 1.25, liked: true });
    await page.goto(`/recordings/${recording}`);
    const draft = page.getByLabel("Track title", { exact: true });
    await draft.fill("Keep this unsaved attachment draft");
    const audio = page.locator("audio");
    await expect(audio).not.toHaveAttribute("src");
    const panel = page.getByRole("region", {
      name: "Add an audio source",
      exact: true,
    });
    await panel
      .getByRole("button", { name: "Find a source", exact: true })
      .click();
    const selector = panel.getByRole("combobox", { name: "Music service" });
    const search = panel.getByRole("searchbox", { name: "Search music" });
    const searchButton = panel.getByRole("button", {
      name: "Search",
      exact: true,
    });
    let firstSources: Schema<"Source">[] = [];
    for (const [index, provider] of providers.entries()) {
      await selector.selectOption(candidates[index].id);
      await search.fill(provider.recording.title);
      if (index === 0)
        await layouts(page, info, "attachment-search", [
          selector,
          search,
          searchButton,
        ]);
      await searchButton.click();
      await expect(
        panel.getByRole("heading", {
          name: provider.recording.title,
          exact: true,
        }),
      ).toBeVisible();
      expect(attachments).toHaveLength(index);
      expect(adoptions).toBe(0);
      const reviewResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/recordings/${recording}/sources/review` &&
          response.request().method() === "POST",
      );
      const reviewButton = panel.getByRole("button", {
        name: "Review match",
        exact: true,
      });
      await reviewButton.click();
      const reviewed = await reviewResponse;
      expect(reviewed.status()).toBe(200);
      const review: Schema<"ProviderSourceReview"> = await reviewed.json();
      const comparison = panel.getByRole("region", {
        name: "Review source match",
        exact: true,
      });
      await expect(comparison.getByText(title, { exact: true })).toBeVisible();
      await expect(comparison.getByText(artist, { exact: true })).toBeVisible();
      await expect(comparison.getByText("2:03", { exact: true })).toBeVisible();
      await expect(
        comparison.getByText(review.candidate.title, { exact: true }),
      ).toBeVisible();
      await expect(comparison.getByText("1:30", { exact: true })).toBeVisible();
      expect(attachments).toHaveLength(index);
      expect(
        retainedCollection(await collection(page, copied), recording),
      ).toEqual(retainedCollection(before, recording));
      expect(
        (
          await get<Schema<"RecordingMetadata">>(
            page,
            `/api/v1/recordings/${recording}/metadata`,
          )
        ).observations.items,
      ).toHaveLength(index + 1);
      expect(providers.flatMap((fixture) => fixture.requests)).toHaveLength(0);
      await expect(draft).toHaveValue("Keep this unsaved attachment draft");
      const confirm = comparison.getByRole("button", {
        name: "Use this source",
        exact: true,
      });
      if (index === 0)
        await layouts(page, info, "attachment-review", [reviewButton, confirm]);
      const attachedResponse = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/recordings/${recording}/sources/attach` &&
          response.request().method() === "POST",
      );
      await confirm.click();
      const attached = await attachedResponse;
      expect(attached.status()).toBe(200);
      const result: Schema<"Recording"> = await attached.json();
      expect(result).toEqual({
        ...primary,
        available: true,
        plays_from_services: true,
      });
      expect(attachments[index]).toEqual({
        candidate: {
          installation_id: candidates[index].id,
          installation_revision: candidates[index].revision,
          provider_recording_id: provider.recording.id,
        },
        review_stamp: review.review_stamp,
      });
      await expect(
        panel.getByText("Source added to this track.", { exact: true }),
      ).toBeVisible();
      await expect(draft).toHaveValue("Keep this unsaved attachment draft");
      await expect(audio).not.toHaveAttribute("src");
      const current = await get<Schema<"RecordingMetadata">>(
        page,
        `/api/v1/recordings/${recording}/metadata`,
      );
      expect(current.title).toEqual(metadata.title);
      expect(current.artist).toEqual(metadata.artist);
      expect(current.selected_observation).toEqual(
        metadata.selected_observation,
      );
      expect(current.observations.items).toHaveLength(index + 2);
      expect(
        retainedCollection(await collection(page, copied), recording),
      ).toEqual(retainedCollection(before, recording));
      const currentCatalog = await get<Schema<"RecordingPage">>(
        page,
        catalogPath,
      );
      expect({
        ...currentCatalog,
        items: currentCatalog.items.map((item) =>
          item.id === recording
            ? { ...item, available: false, plays_from_services: false }
            : item,
        ),
      }).toEqual(catalog);
      if (index === 0) {
        firstSources = await get(
          page,
          `/api/v1/recordings/${recording}/sources`,
        );
        expect(firstSources).toHaveLength(1);
      }
    }
    expect(attachments).toHaveLength(2);
    expect(adoptions).toBe(0);
    const sources = await get<Schema<"Source">[]>(
      page,
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(sources).toHaveLength(2);
    expect(new Set(sources.map((source) => source.id)).size).toBe(2);
    expect(
      sources.every(
        (source) => source.recording_id === recording && source.ready,
      ),
    ).toBe(true);
    const first = firstSources.some((source) => source.id === sources[0].id)
      ? 0
      : 1;
    const alternate = 1 - first;
    const tail = providers[first].holdAudioTail(providers[first].source);
    const availability = page.getByRole("region", {
      name: "Audio availability",
      exact: true,
    });
    const refreshResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
        `/api/v1/recordings/${recording}/sources/refresh`,
    );
    const check = availability.getByRole("button", {
      name: "Check availability",
      exact: true,
    });
    await check.click();
    const refreshed = await refreshResponse;
    expect(refreshed.request().postData()).toBeNull();
    await expect(
      availability.getByText("Audio is ready to play.", { exact: true }),
    ).toBeVisible();
    expect(providers.flatMap((fixture) => fixture.requests)).toHaveLength(0);
    await expect(draft).toHaveValue("Keep this unsaved attachment draft");
    const play = availability.getByRole("button", {
      name: "Play track",
      exact: true,
    });
    await layouts(page, info, "attachment-availability", [
      availability.getByRole("button", { name: "Check again", exact: true }),
      play,
    ]);
    await play.click();
    await expect.poll(() => tail.entered).toBe(true);
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    await expect(audio).toHaveAttribute(
      "src",
      `/api/v1/sources/${sources[0].id}/audio`,
    );
    providers[first].failSource(providers[first].source);
    injectedFailure = true;
    const seek = page.getByRole("slider", { name: "Seek", exact: true });
    await expect
      .poll(async () => Number(await seek.getAttribute("max")))
      .toBeGreaterThan(8);
    await seek.focus();
    await seek.press("PageUp");
    await seek.press("PageUp");
    await seek.press("PageUp");
    const requestedPosition = Number(await seek.inputValue());
    expect(requestedPosition).toBeGreaterThan(8);
    await expect(audio).toHaveAttribute(
      "src",
      `/api/v1/sources/${sources[1].id}/audio`,
    );
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThanOrEqual(requestedPosition - 1);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.currentTime),
    ).toBeLessThan(requestedPosition + 5);
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.paused))
      .toBe(false);
    expect(
      providers[first].requests.some(
        (request) => request.range && /^bytes=[1-9]/.test(request.range),
      ),
    ).toBe(true);
    expect(
      providers[alternate].requests.some(
        (request) => request.source === providers[alternate].source,
      ),
    ).toBe(true);
    await expect(page.locator(".now-playing strong")).toHaveText(title);
    expect(page.url()).toBe(new URL(`/recordings/${recording}`, baseURL).href);
    await expect(draft).toHaveValue("Keep this unsaved attachment draft");
    const afterPlayback = await get<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${copied}`,
    );
    expect(
      afterPlayback.entries.map((entry) => [
        entry.entry_id,
        entry.recording_id,
        entry.position,
        entry.origin,
      ]),
    ).toEqual(
      before.playlist.entries.map((entry) => [
        entry.entry_id,
        entry.recording_id,
        entry.position,
        entry.origin,
      ]),
    );
    await page.getByRole("button", { name: "Next track", exact: true }).click();
    await expect(audio).not.toHaveAttribute("src");
    const username = `attachment-${randomUUID().slice(0, 8)}`;
    const password = "attachment other account password";
    expect(
      (
        await page.request.post("/api/v1/users", {
          headers,
          data: { username, password },
        })
      ).status(),
    ).toBe(201);
    const other = await browser.newContext({
      baseURL,
      storageState: { cookies: [], origins: [] },
    });
    try {
      const listener = await other.newPage();
      listener.on("pageerror", (error) => errors.push(error.message));
      await signIn(listener, username, password);
      const calls = providers.map((provider) => [
        provider.requests.length,
        provider.sourceRequests.length,
      ]);
      const privateHeaders = { ...headers };
      expect(
        (
          await listener.request.get(`/api/v1/recordings/${recording}/sources`)
        ).status(),
      ).toBe(404);
      expect(
        (
          await listener.request.get(`/api/v1/recordings/${recording}/metadata`)
        ).status(),
      ).toBe(404);
      for (const suffix of ["review", "attach"]) {
        expect(
          (
            await listener.request.post(
              `/api/v1/recordings/${recording}/sources/${suffix}`,
              {
                headers: privateHeaders,
                data:
                  suffix === "review"
                    ? attachments[0].candidate
                    : attachments[0],
              },
            )
          ).status(),
        ).toBe(404);
      }
      expect(
        (
          await listener.request.get(`/api/v1/sources/${sources[0].id}/audio`)
        ).status(),
      ).toBe(404);
      expect(
        (await get<Schema<"PluginSettings">>(listener, "/api/v1/plugins"))
          .installations,
      ).toEqual([]);
      expect(
        providers.map((provider) => [
          provider.requests.length,
          provider.sourceRequests.length,
        ]),
      ).toEqual(calls);
    } finally {
      await other.close();
    }
    for (const provider of providers) {
      for (const request of provider.requests) {
        expect(request.authorization).toBe(`Bearer ${provider.token}`);
        expect(request.cookie).toBeUndefined();
        expect(request.source).toBe(provider.source);
      }
    }
    await page.evaluate(() =>
      window.captureAttachmentHistory(
        location.href,
        JSON.stringify(window.history.state) ?? "null",
      ),
    );
    const storage = await page.evaluate(() => ({
      local: Object.entries(localStorage),
      session: Object.entries(sessionStorage),
    }));
    const observations = JSON.stringify({
      urls,
      history,
      storage,
      logs: await Promise.all(logs),
    });
    for (const secret of [
      playlist.token,
      playlist.reference,
      ...providers.flatMap((provider) => [
        provider.token,
        provider.source,
        encodeURIComponent(provider.source),
        provider.address,
      ]),
    ])
      expect(observations).not.toContain(secret);
    expect(errors).toEqual([]);
    // Sign-in probing, absent covers, absent saved copies and the injected audio fault are HTTP diagnostics.
    const knownResponses = new Map([
      [new URL(`/api/v1/recordings/${recording}/artwork`, baseURL).href, 404],
      [new URL(`/api/v1/sources/${sources[0].id}/audio`, baseURL).href, 502],
    ]);
    const absentCopy = new RegExp(
      `^${new URL("/api/v1/", baseURL).href}(recordings|playlists)/[^/]+/(artwork|download)$`,
    );
    const expectedResponses = {
      get: (url: string) =>
        knownResponses.get(url) ?? (absentCopy.test(url) ? 404 : undefined),
    };
    const messages = new Map([
      [
        401,
        "Failed to load resource: the server responded with a status of 401 (Unauthorized)",
      ],
      [
        404,
        "Failed to load resource: the server responded with a status of 404 (Not Found)",
      ],
      [
        502,
        "Failed to load resource: the server responded with a status of 502 (Bad Gateway)",
      ],
    ]);
    for (const response of failedResponses) {
      expect(response.method).toBe("GET");
      expect(response.status).toBe(expectedResponses.get(response.url));
      if (response.status === 401) expect(response.beforeSignIn).toBe(true);
      if (response.status === 502)
        expect(response.afterInjectedFailure).toBe(true);
    }
    expect(
      failedResponses.filter((response) => response.status === 401),
    ).toHaveLength(0);
    expect(
      failedResponses.filter((response) => response.status === 502),
    ).toHaveLength(1);
    expect(failedResponses.some((response) => response.status === 404)).toBe(
      true,
    );
    const correlated = new Set<number>();
    const unexpectedErrors = consoleErrors.filter((diagnostic) => {
      const status = expectedResponses.get(diagnostic.url);
      if (status === undefined || diagnostic.text !== messages.get(status))
        return true;
      const index = failedResponses.findIndex(
        (response, index) =>
          !correlated.has(index) &&
          response.url === diagnostic.url &&
          response.status === status &&
          response.method === "GET",
      );
      if (index === -1) return true;
      correlated.add(index);
      return false;
    });
    expect(unexpectedErrors).toEqual([]);
    expect(correlated.size).toBe(failedResponses.length);
  } finally {
    for (const connection of connections)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    if (copied)
      await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "attachment-e2e-cleanup",
          operations: [
            {
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id: copied },
            },
          ],
        } satisfies Schema<"SyncBatch">,
      });
    for (const service of services)
      await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
        headers,
        data: { revision: service.revision },
      });
    await Promise.all([
      playlist.close(),
      ...providers.map((provider) => provider.close()),
    ]);
  }
});
