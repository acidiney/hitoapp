import { randomUUID } from "node:crypto";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import { expect, test, type Page } from "@playwright/test";
import { playlistFixture } from "./fixtures/playlist";
import type { Schema } from "../src/shared/schema";

declare global {
  interface Window {
    captureNewCopyHistory: (url: string, state: unknown) => Promise<void>;
  }
}

async function publicState(page: Page) {
  const responses = await Promise.all([
    page.request.get("/api/v1/playlists"),
    page.request.get("/api/v1/recordings"),
    page.request.get("/api/v1/sync?cursor=0"),
  ]);
  for (const response of responses) expect(response.status()).toBe(200);
  return {
    playlists: (await responses[0].json()) as Schema<"Playlist">[],
    catalog: (await responses[1].json()) as Schema<"RecordingPage">,
    feed: (await responses[2].json()) as Schema<"SyncPage">,
  };
}

test("a new copy uses the current field after a committed import loses its response", async ({
  page,
  baseURL,
}) => {
  if (!baseURL)
    throw new Error("The isolated e2e server URL must be configured");
  const fixture = await playlistFixture();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const errors: string[] = [];
  const urls: string[] = [];
  const logs: Promise<string>[] = [];
  const history: unknown[] = [];
  const imports: string[] = [];
  const copies: string[] = [];
  let service: Schema<"PluginService"> | null = null;
  let connection: Schema<"PluginInstallation"> | null = null;
  const lost: {
    receipt?: Schema<"PlaylistImportReceipt">;
    body?: string;
  } = {};
  let forwarded = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) =>
    logs.push(
      Promise.all(message.args().map((argument) => argument.jsonValue()))
        .then((values) => JSON.stringify(values))
        .catch(() => message.text()),
    ),
  );
  page.on("request", (request) => {
    urls.push(request.url());
    if (
      /\/plugins\/[^/]+\/playlists\/import$/.test(
        new URL(request.url()).pathname,
      )
    ) {
      const body = request.postData();
      if (body !== null) imports.push(body);
    }
  });
  await page.exposeFunction(
    "captureNewCopyHistory",
    (url: string, state: unknown) => {
      history.push({ url, state });
    },
  );
  await page.addInitScript(() => {
    localStorage.setItem("hito-log-level", "debug");
    for (const method of ["pushState", "replaceState"] as const) {
      const original = window.history[method].bind(window.history);
      window.history[method] = (state, unused, url) => {
        void window.captureNewCopyHistory(String(url ?? location.href), state);
        original(state, unused, url);
      };
    }
  });
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: `${fixture.prefix} new copy`,
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
    });
    expect(offered.status()).toBe(201);
    service = await offered.json();
    if (!service)
      throw new Error("Offering the fixture must return its service");
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema<"InstallPlugin">,
    });
    expect(installed.status()).toBe(201);
    connection = await installed.json();
    if (!connection)
      throw new Error("Connecting the fixture must return its installation");
    const importPath = `/api/v1/plugins/${connection.id}/playlists/import`;
    const importURL = new URL(importPath, baseURL).href;
    const before = await publicState(page);
    expect(before.feed.has_more).toBe(false);
    await page.route(
      importURL,
      async (route) => {
        forwarded++;
        const body = route.request().postData();
        if (body === null)
          throw new Error("The original import must send a JSON body");
        lost.body = body;
        try {
          const response = await route.fetch({ maxRetries: 0 });
          expect(response.status()).toBe(200);
          lost.receipt = JSON.parse((await response.body()).toString());
          if (!lost.receipt)
            throw new Error(
              "The real import must return its committed receipt",
            );
          copies.push(lost.receipt.playlist_id);
          await response.dispose();
        } finally {
          await route.abort("failed");
        }
      },
      { times: 1 },
    );
    await page.goto("/playlists");
    await openPlaylistImport(page);
    const form = page.getByRole("region", { name: "Import a playlist" });
    const field = form.getByRole("textbox", {
      name: "Playlist link or identifier",
    });
    await field.fill(fixture.reference);
    const failed = page.waitForEvent("requestfailed", {
      predicate: (request) =>
        request.url() === importURL && request.method() === "POST",
    });
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    await failed;
    await expect(form.getByRole("alert")).toBeVisible();
    await expect(page).toHaveURL(new URL("/playlists", baseURL).href);
    await expect(
      form.getByRole("button", { name: "Retry import", exact: true }),
    ).toBeEnabled();
    expect(forwarded).toBe(1);
    if (!lost.receipt || !lost.body)
      throw new Error("The real import must commit before delivery fails");
    const original: Schema<"ImportProviderPlaylist"> = JSON.parse(lost.body);
    expect(original).toEqual({
      request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      installation_revision: connection.revision,
      playlist_reference: fixture.reference,
    });
    expect(lost.receipt).toEqual({
      request_id: original.request_id,
      playlist_id: copies[0],
    });
    expect(imports).toEqual([lost.body]);
    const committed = await publicState(page);
    expect(committed.playlists).toHaveLength(before.playlists.length + 1);
    expect(committed.catalog.total).toBe(before.catalog.total + 1);
    expect(committed.feed.has_more).toBe(false);
    expect(committed.feed.operations).toHaveLength(
      before.feed.operations.length + 1,
    );
    expect(fixture.requests).toHaveLength(4);
    const settingsResponse = await page.request.get("/api/v1/plugins");
    expect(settingsResponse.status()).toBe(200);
    const settings: Schema<"PluginSettings"> = await settingsResponse.json();
    expect(
      settings.installations.find((item) => item.id === connection?.id)
        ?.revision,
    ).toBe(connection.revision);
    await field.fill(fixture.secondReference);
    await expect(field).toHaveValue(fixture.secondReference);
    expect(fixture.secondReference).not.toBe(fixture.reference);
    expect(imports).toEqual([lost.body]);
    const imported = page.waitForResponse(
      (response) =>
        response.url() === importURL && response.request().method() === "POST",
    );
    await form
      .getByRole("button", { name: "Import a new copy", exact: true })
      .click();
    const response = await imported;
    expect(response.status()).toBe(200);
    const replacement: Schema<"ImportProviderPlaylist"> = response
      .request()
      .postDataJSON();
    expect(replacement).toEqual({
      request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      installation_revision: connection.revision,
      playlist_reference: fixture.secondReference,
    });
    expect(replacement.request_id).not.toBe(original.request_id);
    const receipt: Schema<"PlaylistImportReceipt"> = await response.json();
    copies.push(receipt.playlist_id);
    expect(receipt).toEqual({
      request_id: replacement.request_id,
      playlist_id: copies[1],
    });
    expect(copies[1]).not.toBe(copies[0]);
    expect(imports).toEqual([lost.body, response.request().postData()]);
    expect(forwarded).toBe(1);
    await expect(page).toHaveURL(
      new URL(`/playlists/${copies[1]}`, baseURL).href,
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
    const completed = await publicState(page);
    expect(completed.playlists).toHaveLength(before.playlists.length + 2);
    expect(completed.playlists.map((playlist) => playlist.id)).toEqual(
      expect.arrayContaining(copies),
    );
    expect(completed.catalog).toEqual(committed.catalog);
    expect(completed.feed.has_more).toBe(false);
    expect(completed.feed.operations).toHaveLength(
      before.feed.operations.length + 2,
    );
    expect(
      completed.feed.operations.slice(0, committed.feed.operations.length),
    ).toEqual(committed.feed.operations);
    const publications = completed.feed.operations.slice(
      before.feed.operations.length,
    );
    expect(
      new Set(
        publications.map((event) =>
          "publication" in event ? event.publication.publication_id : null,
        ),
      ).size,
    ).toBe(2);
    const details: Schema<"PlaylistDetail">[] = [];
    for (const [index, id] of copies.entries()) {
      const detailResponse = await page.request.get(`/api/v1/playlists/${id}`);
      expect(detailResponse.status()).toBe(200);
      const detail: Schema<"PlaylistDetail"> = await detailResponse.json();
      details.push(detail);
      expect(detail.playlist).toEqual({
        id,
        kind: "local",
        name: fixture.detail.name,
      });
      expect(detail.origin?.attribution).toEqual(fixture.detail.attribution);
      expect(
        detail.entries.map((entry) => [
          entry.position,
          entry.origin?.occurrence_id,
          entry.origin?.kind,
        ]),
      ).toEqual([
        [0, "first", "recording"],
        [1, "withdrawn", "unavailable"],
        [2, "unknown", "unknown"],
        [3, "duplicate", "recording"],
      ]);
      expect(detail.entries.every((entry) => !entry.available)).toBe(true);
      expect(detail.entries[0].recording_id).toBeTruthy();
      expect(detail.entries[3].recording_id).toBe(
        detail.entries[0].recording_id,
      );
      expect(detail.entries[1].recording_id).toBeNull();
      expect(detail.entries[2].recording_id).toBeNull();
      expect(publications[index]).toMatchObject({
        publication: {
          kind: "playlist_imported",
          snapshot: {
            playlist_id: id,
            entries: detail.entries.map((entry) => ({
              entry_id: entry.entry_id,
              position: entry.position,
              origin: entry.origin,
            })),
          },
        },
      });
    }
    expect(
      new Set(
        details.flatMap((detail) =>
          detail.entries.map((entry) => entry.entry_id),
        ),
      ).size,
    ).toBe(8);
    expect(details[1].entries.map((entry) => entry.recording_id)).toEqual(
      details[0].entries.map((entry) => entry.recording_id),
    );
    expect(
      fixture.requests.slice(4).map(({ path, body }) => [path, body]),
    ).toEqual([
      [
        "/fixture/v1/playlists/detail",
        { kind: "reference", reference: fixture.secondReference },
      ],
      [
        "/fixture/v1/playlists/items",
        {
          playlist_id: fixture.detail.id,
          snapshot: fixture.detail.snapshot,
          cursor: null,
          limit: 100,
        },
      ],
      [
        "/fixture/v1/playlists/detail",
        { kind: "canonical", playlist_id: fixture.detail.id },
      ],
    ]);
    const providerRequests = fixture.requests.length;
    fixture.offline();
    const replay = await page.request.post(importPath, {
      headers: { ...headers, "content-type": "application/json" },
      data: lost.body,
    });
    expect(replay.status()).toBe(200);
    expect(await replay.json()).toEqual(lost.receipt);
    expect(await publicState(page)).toEqual(completed);
    for (const [index, id] of copies.entries()) {
      const unchanged = await page.request.get(`/api/v1/playlists/${id}`);
      expect(unchanged.status()).toBe(200);
      expect(await unchanged.json()).toEqual(details[index]);
    }
    expect(fixture.requests).toHaveLength(providerRequests);
    const storage = await page.evaluate(() => ({
      local: Object.entries(localStorage),
      session: Object.entries(sessionStorage),
      state: window.history.state,
    }));
    const observations = JSON.stringify({
      urls,
      logs: await Promise.all(logs),
      history,
      storage,
      url: page.url(),
    });
    for (const secret of [
      fixture.token,
      fixture.reference,
      fixture.secondReference,
      encodeURIComponent(fixture.reference),
      encodeURIComponent(fixture.secondReference),
    ])
      expect(observations).not.toContain(secret);
    expect(errors).toEqual([]);
  } finally {
    try {
      if (connection)
        await page.request.delete(`/api/v1/plugins/${connection.id}`, {
          headers,
          data: { revision: connection.revision },
        });
      if (copies.length)
        await page.request.post("/api/v1/sync", {
          headers,
          data: {
            device_id: "playlist-new-copy-e2e-cleanup",
            operations: copies.map((playlist_id) => ({
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id },
            })),
          } satisfies Schema<"SyncBatch">,
        });
      if (service)
        await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
          headers,
          data: { revision: service.revision },
        });
    } finally {
      await fixture.close();
    }
  }
});
