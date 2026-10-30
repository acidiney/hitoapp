import { randomUUID } from "node:crypto";
import {
  expect,
  test,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import { playlistFixture } from "./fixtures/playlist";
import type { Schema } from "../src/shared/schema";
import { enterEditMode } from "./fixtures/playlist-edit";
type Target = {
  label: string;
  width: number;
  height: number;
  viewport: number;
};

declare global {
  interface Window {
    capturePlaylistHistory: (url: string, state: string) => Promise<void>;
  }
}

async function signIn(page: Page) {
  await page.goto("/library");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

async function layouts(
  page: Page,
  info: TestInfo,
  name: string,
  controls: Locator[],
  targets: Target[],
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
      const box = await control.boundingBox();
      const dock = await page
        .getByRole("contentinfo", { name: "Audio player" })
        .boundingBox();
      if (!box || !dock)
        throw new Error(
          "Playlist controls and player dock need visible bounds",
        );
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
      expect(box.y + box.height).toBeLessThanOrEqual(dock.y);
      expect(
        await control.evaluate((element) => {
          const bounds = element.getBoundingClientRect();
          const hit = document.elementFromPoint(
            bounds.x + bounds.width / 2,
            bounds.y + bounds.height / 2,
          );
          return hit !== null && element.contains(hit);
        }),
      ).toBe(true);
      targets.push({
        label:
          (await control.getAttribute("aria-label")) ??
          (await control.innerText()),
        width: box.width,
        height: box.height,
        viewport: viewport.width,
      });
    }
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: info.outputPath(`${name}-${viewport.width}.png`),
    });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
}

test("a provider playlist becomes a durable editable metadata copy without phantom playback", async ({
  page,
  baseURL,
}, info) => {
  if (!baseURL)
    throw new Error("The isolated e2e server URL must be configured");
  const fixture = await playlistFixture();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const errors: string[] = [];
  const urls: string[] = [];
  const history: { url: string; state: string }[] = [];
  const logs: Promise<string>[] = [];
  const targets: Target[] = [];
  let importRequests = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    urls.push(request.url());
    if (
      /\/plugins\/[^/]+\/playlists\/import$/.test(
        new URL(request.url()).pathname,
      )
    )
      importRequests++;
  });
  page.on("console", (message) =>
    logs.push(
      Promise.all(message.args().map((argument) => argument.jsonValue()))
        .then((values) => JSON.stringify(values))
        .catch(() => message.text()),
    ),
  );
  await page.exposeFunction(
    "capturePlaylistHistory",
    (url: string, state: string) => {
      history.push({ url, state });
    },
  );
  await page.addInitScript(() => {
    localStorage.setItem("hito-log-level", "debug");
    for (const method of ["pushState", "replaceState"] as const) {
      const original = window.history[method].bind(window.history);
      window.history[method] = (state, unused, url) => {
        void window.capturePlaylistHistory(
          String(url ?? location.href),
          JSON.stringify(state) ?? "null",
        );
        original(state, unused, url);
      };
    }
  });
  let service: Schema<"PluginService"> | null = null;
  let connection: Schema<"PluginInstallation"> | null = null;
  let copied: string | null = null;
  try {
    await signIn(page);
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: `${fixture.prefix} collection`,
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
    });
    expect(offered.status()).toBe(201);
    service = await offered.json();
    if (!service)
      throw new Error("Offering the playlist fixture must return its service");
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
      throw new Error(
        "Connecting the playlist fixture must return its installation",
      );
    const connectionId = connection.id;
    const importPath = `/api/v1/plugins/${connectionId}/playlists/import`;
    await page.goto("/playlists");
    const form = page.getByRole("region", { name: "Import a playlist" });
    const selector = form.getByRole("combobox", { name: "Playlist service" });
    const reference = form.getByRole("textbox", {
      name: "Playlist link or identifier",
    });
    const submit = form.getByRole("button", {
      name: "Import a copy",
      exact: true,
    });
    await selector.selectOption(connection.id);
    await reference.fill(`  ${fixture.reference}  `);
    expect(fixture.requests).toHaveLength(1);
    expect(importRequests).toBe(0);
    await expect(form).toContainText("without downloading audio");
    await layouts(
      page,
      info,
      "playlist-import-form",
      [selector, reference, submit],
      targets,
    );
    await reference.focus();
    await expect(reference).toBeFocused();
    await reference.press("Tab");
    await expect(submit).toBeFocused();
    const importResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === importPath &&
        response.request().method() === "POST",
    );
    await submit.press("Enter");
    const imported = await importResponse;
    expect(imported.status()).toBe(200);
    expect(importRequests).toBe(1);
    const sent: Schema<"ImportProviderPlaylist"> = imported
      .request()
      .postDataJSON();
    expect(sent).toEqual({
      request_id: expect.stringMatching(/^[0-9a-f-]{36}$/),
      installation_revision: connection.revision,
      playlist_reference: fixture.reference,
    });
    const receipt: Schema<"PlaylistImportReceipt"> = await imported.json();
    expect(receipt.request_id).toBe(sent.request_id);
    expect(Object.keys(receipt).sort()).toEqual(["playlist_id", "request_id"]);
    copied = receipt.playlist_id;
    const copyPath = `/playlists/${copied}`;
    const detailPath = `/api/v1/playlists/${copied}`;
    await expect(page).toHaveURL(new URL(copyPath, baseURL).href);
    await expect(
      page.getByRole("heading", { name: fixture.detail.name, exact: true }),
    ).toBeVisible();
    const detailResponse = await page.request.get(detailPath);
    expect(detailResponse.status()).toBe(200);
    const detail: Schema<"PlaylistDetail"> = await detailResponse.json();
    expect(detail.playlist).toEqual({
      id: copied,
      kind: "local",
      name: fixture.detail.name,
    });
    expect(detail.origin?.attribution.label).toBe(
      fixture.detail.attribution.label,
    );
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
    expect(new Set(detail.entries.map((entry) => entry.entry_id)).size).toBe(4);
    expect(detail.entries.every((entry) => !entry.available)).toBe(true);
    const recording = detail.entries[0].recording_id;
    if (!recording)
      throw new Error("Known metadata must have a real recording identity");
    expect(detail.entries[3].recording_id).toBe(recording);
    expect(detail.entries[1].recording_id).toBeNull();
    expect(detail.entries[2].recording_id).toBeNull();
    const sources = await page.request.get(
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(sources.status()).toBe(200);
    expect(await sources.json()).toEqual([]);
    const table = page.getByRole("table", { name: "Playlist entries" });
    const rows = table.locator("tbody").getByRole("row");
    await expect(rows).toHaveCount(4);
    await expect(rows.locator(".track-title, .retained-title")).toHaveText([
      fixture.recording.title,
      fixture.withdrawn,
      "Unknown provider item",
      fixture.recording.title,
    ]);
    await expect(
      page.getByRole("button", { name: "Play playlist", exact: true }),
    ).toBeEnabled();
    for (const index of [0, 3]) {
      await expect(
        rows.nth(index).getByRole("button", {
          name: `Play ${fixture.recording.title}`,
          exact: true,
        }),
      ).toBeEnabled();
      await expect(
        rows
          .nth(index)
          .getByRole("button", { name: fixture.recording.title, exact: true }),
      ).toBeEnabled();
      await expect(
        rows
          .nth(index)
          .getByText("Unavailable on this device", { exact: true }),
      ).toBeVisible();
    }
    for (const [index, status] of [
      [1, "Unavailable from its provider"],
      [2, "Unknown item from its provider"],
    ] as const) {
      await expect(
        rows.nth(index).getByText(status, { exact: true }),
      ).toBeVisible();
      await expect(rows.nth(index).getByRole("link")).toHaveCount(0);
      await expect(rows.nth(index).getByRole("button")).toHaveCount(0);
      await expect(rows.nth(index).locator("img")).toHaveCount(0);
      await expect(rows.nth(index).locator(".duration-column")).toHaveText("—");
    }
    await expect(
      page.getByText(fixture.detail.attribution.label, { exact: true }),
    ).toBeVisible();
    await expect(page.locator("audio")).not.toHaveAttribute("src");
    expect(
      fixture.requests.map(({ method, path, body }) => [method, path, body]),
    ).toEqual([
      ["GET", "/fixture/v1/manifest", null],
      [
        "POST",
        "/fixture/v1/playlists/detail",
        { kind: "reference", reference: fixture.reference },
      ],
      [
        "POST",
        "/fixture/v1/playlists/items",
        {
          playlist_id: fixture.detail.id,
          snapshot: fixture.detail.snapshot,
          cursor: null,
          limit: 100,
        },
      ],
      [
        "POST",
        "/fixture/v1/playlists/detail",
        { kind: "canonical", playlist_id: fixture.detail.id },
      ],
    ]);
    await enterEditMode(page);
    const observedRequests = fixture.requests.length;
    await layouts(
      page,
      info,
      "playlist-import-entries",
      [
        ...(await rows.getByRole("button", { name: /^Remove / }).all()),
        ...(await rows.getByRole("button", { name: /^Like / }).all()),
        ...(await rows.getByRole("link", { name: /^Details for / }).all()),
        page.getByRole("button", { name: "More options", exact: true }),
      ],
      targets,
    );
    fixture.offline();
    await page.reload();
    await expect(rows).toHaveCount(4);
    expect(await (await page.request.get(detailPath)).json()).toEqual(detail);
    await page.goto("/settings?section=plugins");
    const plugin = page.getByRole("listitem").filter({
      has: page.getByRole("heading", {
        name: fixture.manifest.name,
        exact: true,
      }),
    });
    await plugin.getByRole("button", { name: "Remove", exact: true }).click();
    const removePlugin = plugin.getByRole("button", {
      name: "Remove plugin",
      exact: true,
    });
    await removePlugin.focus();
    await expect(removePlugin).toBeFocused();
    const uninstall = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/plugins/${connectionId}` &&
        response.request().method() === "DELETE",
    );
    await removePlugin.press("Enter");
    expect((await uninstall).status()).toBe(204);
    connection = null;
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeFocused();
    await page.goto(copyPath);
    await expect(rows).toHaveCount(4);
    expect(await (await page.request.get(detailPath)).json()).toEqual(detail);
    await enterEditMode(page);
    const remaining = [...detail.entries];
    for (const entry of [detail.entries[0], detail.entries[2]]) {
      const index = remaining.findIndex(
        (item) => item.entry_id === entry.entry_id,
      );
      const remove = rows.nth(index).getByRole("button", {
        name: `Remove ${entry.title} from playlist`,
        exact: true,
      });
      await remove.focus();
      await expect(remove).toBeFocused();
      const changed = page.waitForRequest(
        (request) =>
          new URL(request.url()).pathname === "/api/v1/sync" &&
          request.method() === "POST",
      );
      await remove.press("Enter");
      const batch: Schema<"SyncBatch"> = (await changed).postDataJSON();
      expect(batch.operations).toEqual([
        {
          id: expect.any(String),
          action: {
            kind: "entry_remove",
            playlist_id: copied,
            entry_id: entry.entry_id,
          },
        },
      ]);
      remaining.splice(index, 1);
      await expect(rows).toHaveCount(remaining.length);
      expect(await (await page.request.get(detailPath)).json()).toEqual({
        ...detail,
        entries: remaining,
      });
    }
    await page.reload();
    await expect(rows).toHaveCount(2);
    await expect(
      rows
        .last()
        .getByRole("button", { name: fixture.recording.title, exact: true }),
    ).toBeEnabled();
    await expect(
      page.getByText(fixture.detail.attribution.label, { exact: true }),
    ).toBeVisible();
    expect(fixture.requests).toHaveLength(observedRequests);
    expect(importRequests).toBe(1);
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
    }
    await page
      .getByRole("button", { name: "More options", exact: true })
      .click();
    await page
      .getByRole("menuitem", { name: "Delete playlist", exact: true })
      .click();
    await expect(page).toHaveURL(new URL("/playlists", baseURL).href);
    expect((await page.request.get(detailPath)).status()).toBe(404);
    copied = null;
    await page.evaluate(async () =>
      window.capturePlaylistHistory(
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
      captured.some((message) => message.includes("playlist_import.completed")),
    ).toBe(true);
    const observations = JSON.stringify({
      urls,
      history,
      storage,
      logs: captured,
      url: page.url(),
    });
    for (const secret of [
      fixture.token,
      fixture.reference,
      encodeURIComponent(fixture.reference),
    ])
      expect(observations).not.toContain(secret);
    expect(errors).toEqual([]);
    for (const target of targets) {
      expect(
        target.width,
        `${target.viewport}px ${target.label} width`,
      ).toBeGreaterThanOrEqual(44);
      expect(
        target.height,
        `${target.viewport}px ${target.label} height`,
      ).toBeGreaterThanOrEqual(44);
    }
  } finally {
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    if (copied)
      await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "playlist-e2e-cleanup",
          operations: [
            {
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id: copied },
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
