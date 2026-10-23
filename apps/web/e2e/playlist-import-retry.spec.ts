import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import { playlistFixture } from "./fixtures/playlist";
import type { components } from "../src/generated";
type Schema = components["schemas"];
type PublicState = {
  playlists: Schema["Playlist"][];
  catalog: Schema["RecordingPage"];
  feed: Schema["SyncPage"];
};

declare global {
  interface Window {
    capturePlaylistRetryHistory: (url: string, state: string) => Promise<void>;
  }
}

async function publicState(page: Page): Promise<PublicState> {
  const responses = await Promise.all([
    page.request.get("/api/v1/playlists"),
    page.request.get("/api/v1/recordings"),
    page.request.get("/api/v1/sync?cursor=0"),
  ]);
  for (const response of responses) expect(response.status()).toBe(200);
  return {
    playlists: await responses[0].json(),
    catalog: await responses[1].json(),
    feed: await responses[2].json(),
  };
}

test("a committed but lost import retries its original body while preserving an unsent draft", async ({
  page,
  baseURL,
}) => {
  if (!baseURL)
    throw new Error("The isolated e2e server URL must be configured");
  const fixture = await playlistFixture();
  const draft = `https://provider.invalid/private-unsent-draft/${randomUUID()}`;
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const errors: string[] = [];
  const urls: string[] = [];
  const historyCaptures: { url: string; state: string }[] = [];
  const logs: Promise<string>[] = [];
  const transmitted: string[] = [];
  const importBodies: string[] = [];
  const lost: {
    receipt?: Schema["PlaylistImportReceipt"];
    serialized?: string;
    forwarded: number;
    bodyCompleted: boolean;
  } = { forwarded: 0, bodyCompleted: false };
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    urls.push(request.url());
    const body = request.postData();
    if (body !== null) {
      transmitted.push(body);
      if (
        /\/plugins\/[^/]+\/playlists\/import$/.test(
          new URL(request.url()).pathname,
        )
      )
        importBodies.push(body);
    }
  });
  page.on("console", (message) =>
    logs.push(
      Promise.all(message.args().map((argument) => argument.jsonValue()))
        .then((values) => JSON.stringify(values))
        .catch(() => message.text()),
    ),
  );
  await page.exposeFunction(
    "capturePlaylistRetryHistory",
    (url: string, state: string) => {
      historyCaptures.push({ url, state });
    },
  );
  await page.addInitScript(() => {
    localStorage.setItem("hito-log-level", "debug");
    for (const method of ["pushState", "replaceState"] as const) {
      const original = window.history[method].bind(window.history);
      window.history[method] = (state, unused, url) => {
        void window.capturePlaylistRetryHistory(
          String(url ?? location.href),
          JSON.stringify(state) ?? "null",
        );
        original(state, unused, url);
      };
    }
  });
  let service: Schema["PluginService"] | null = null;
  let connection: Schema["PluginInstallation"] | null = null;
  let copy: string | null = null;
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: `${fixture.prefix} replay collection`,
        base_url: fixture.address,
      } satisfies Schema["OfferPluginService"],
    });
    expect(offered.status()).toBe(201);
    service = await offered.json();
    if (!service)
      throw new Error("Offering the replay fixture must return its service");
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema["InstallPlugin"],
    });
    expect(installed.status()).toBe(201);
    connection = await installed.json();
    if (!connection)
      throw new Error(
        "Connecting the replay fixture must return its installation",
      );
    const importPath = `/api/v1/plugins/${connection.id}/playlists/import`;
    const importURL = new URL(importPath, baseURL).href;
    const before = await publicState(page);
    expect(before.feed.has_more).toBe(false);
    await page.route(
      importURL,
      async (route) => {
        lost.forwarded++;
        const serialized = route.request().postData();
        if (serialized === null)
          throw new Error("An import request must have its original JSON body");
        lost.serialized = serialized;
        try {
          const response = await route.fetch({ maxRetries: 0 });
          expect(response.status()).toBe(200);
          const completed = await response.body();
          lost.bodyCompleted = true;
          lost.receipt = JSON.parse(completed.toString());
          await response.dispose();
        } finally {
          await route.abort("failed");
        }
      },
      { times: 1 },
    );
    await page.goto("/playlists");
    const form = page.getByRole("region", { name: "Import a playlist" });
    await form
      .getByRole("combobox", { name: "Playlist service" })
      .selectOption(connection.id);
    const field = form.getByRole("textbox", {
      name: "Playlist link or identifier",
    });
    await field.fill(fixture.reference);
    expect(importBodies).toEqual([]);
    const failedDelivery = page.waitForEvent("requestfailed", {
      predicate: (request) =>
        request.url() === importURL && request.method() === "POST",
    });
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    await failedDelivery;
    await expect(form.getByRole("alert")).toBeVisible();
    await expect(page).toHaveURL(new URL("/playlists", baseURL).href);
    await expect(
      form.getByRole("button", { name: "Retry import", exact: true }),
    ).toBeEnabled();
    expect(lost.forwarded).toBe(1);
    expect(lost.bodyCompleted).toBe(true);
    if (!lost.receipt || !lost.serialized)
      throw new Error(
        "The real server response must commit before client delivery is aborted",
      );
    const original: Schema["ImportProviderPlaylist"] = JSON.parse(
      lost.serialized,
    );
    expect(original).toEqual({
      request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      installation_revision: connection.revision,
      playlist_reference: fixture.reference,
    });
    expect(lost.receipt.request_id).toBe(original.request_id);
    expect(Object.keys(lost.receipt).sort()).toEqual([
      "playlist_id",
      "request_id",
    ]);
    expect(importBodies).toEqual([lost.serialized]);
    copy = lost.receipt.playlist_id;
    const detailPath = `/api/v1/playlists/${copy}`;
    const detailResponse = await page.request.get(detailPath);
    expect(detailResponse.status()).toBe(200);
    const detail: Schema["PlaylistDetail"] = await detailResponse.json();
    expect(detail.playlist.id).toBe(copy);
    expect(detail.entries).toHaveLength(4);
    expect(detail.entries.map((entry) => entry.origin?.occurrence_id)).toEqual([
      "first",
      "withdrawn",
      "unknown",
      "duplicate",
    ]);
    expect(new Set(detail.entries.map((entry) => entry.entry_id)).size).toBe(4);
    const committed = await publicState(page);
    expect(committed.playlists).toHaveLength(before.playlists.length + 1);
    expect(committed.catalog.total).toBe(before.catalog.total + 1);
    expect(committed.feed.has_more).toBe(false);
    expect(committed.feed.operations).toHaveLength(
      before.feed.operations.length + 1,
    );
    expect(
      committed.feed.operations.filter(
        (event) =>
          "publication" in event &&
          event.publication.kind === "playlist_imported" &&
          event.publication.snapshot.playlist_id === copy,
      ),
    ).toHaveLength(1);
    expect(fixture.requests.map(({ path }) => path)).toEqual([
      "/fixture/v1/manifest",
      "/fixture/v1/playlists/detail",
      "/fixture/v1/playlists/items",
      "/fixture/v1/playlists/detail",
    ]);
    const observed = fixture.requests.length;
    await field.fill(draft);
    await expect(field).toHaveValue(draft);
    await expect(
      form.getByRole("button", { name: "Import a new copy", exact: true }),
    ).toBeEnabled();
    expect(importBodies).toEqual([lost.serialized]);
    fixture.offline();
    const retryResponse = page.waitForResponse(
      (response) =>
        response.url() === importURL && response.request().method() === "POST",
    );
    await form
      .getByRole("button", { name: "Retry import", exact: true })
      .click();
    const replay = await retryResponse;
    expect(replay.status()).toBe(200);
    const receipt: Schema["PlaylistImportReceipt"] = await replay.json();
    expect(receipt).toEqual(lost.receipt);
    expect(replay.request().postData()).toBe(lost.serialized);
    expect(importBodies).toEqual([lost.serialized, lost.serialized]);
    expect(lost.forwarded).toBe(1);
    await expect(page).toHaveURL(
      new URL(`/playlists/${receipt.playlist_id}`, baseURL).href,
    );
    await expect(
      page.getByRole("heading", { name: fixture.detail.name, exact: true }),
    ).toBeVisible();
    await expect(
      page
        .getByRole("table", { name: "Playlist entries" })
        .locator("tbody")
        .getByRole("row"),
    ).toHaveCount(4);
    expect(await publicState(page)).toEqual(committed);
    const replayedDetail = await page.request.get(detailPath);
    expect(replayedDetail.status()).toBe(200);
    expect(await replayedDetail.json()).toEqual(detail);
    expect(fixture.requests).toHaveLength(observed);
    expect(transmitted.every((body) => !body.includes(draft))).toBe(true);
    expect(
      JSON.stringify(fixture.requests.map(({ body }) => body)),
    ).not.toContain(draft);
    await page.evaluate(async () =>
      window.capturePlaylistRetryHistory(
        location.href,
        JSON.stringify(window.history.state) ?? "null",
      ),
    );
    const storage = await page.evaluate(() => ({
      local: Object.entries(localStorage),
      session: Object.entries(sessionStorage),
    }));
    const captured = await Promise.all(logs);
    expect(
      captured.some((message) => message.includes("playlist_import.failed")),
    ).toBe(true);
    expect(
      captured.some((message) => message.includes("playlist_import.completed")),
    ).toBe(true);
    const observations = JSON.stringify({
      urls,
      history: historyCaptures,
      storage,
      logs: captured,
      url: page.url(),
    });
    for (const secret of [
      fixture.reference,
      draft,
      fixture.token,
      encodeURIComponent(fixture.reference),
      encodeURIComponent(draft),
    ])
      expect(observations).not.toContain(secret);
    expect(errors).toEqual([]);
  } finally {
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    if (copy)
      await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "playlist-retry-e2e-cleanup",
          operations: [
            {
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id: copy },
            },
          ],
        },
      });
    if (service)
      await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
        headers,
        data: { revision: service.revision },
      });
    await fixture.close();
  }
});
