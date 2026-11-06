import { randomUUID } from "node:crypto";
import { chooseOption } from "./fixtures/listbox";
import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import { artworkFixture } from "./fixtures/artwork";
import {
  get,
  layouts,
  loadedCopies,
  savedState,
  savePersonalState,
  signIn,
} from "./fixtures/artwork-browser-checks";
import { artworkDiagnostics, type Phase } from "./fixtures/artwork-diagnostics";
import type { Schema } from "../src/shared/schema";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";

test("existing independent playlist copies automatically retain private provider artwork without changing saved music", async ({
  page,
  browser,
  baseURL,
}, info) => {
  if (!baseURL)
    throw new Error("The isolated e2e release server URL must be configured");
  const fixture = await artworkFixture();
  const origin = new URL(baseURL).origin;
  const headers = { "x-hito-request": "1", origin };
  const copies: string[] = [];
  const availability: { phase: Phase; value: Schema<"ArtworkAvailability"> }[] =
    [];
  const jobs: {
    phase: Phase;
    method: string;
    path: string;
    value: Schema<"ArtworkFetch">;
  }[] = [];
  let phase: Phase = "sign-in";
  let service: Schema<"PluginService"> | null = null;
  let connection: Schema<"PluginInstallation"> | null = null;
  let recording: string | undefined;
  const diagnostics = artworkDiagnostics(origin, () => phase);
  const { imports, artworkEnqueues, observe } = diagnostics;
  observe(page, "owner");
  await page.addInitScript(() =>
    localStorage.setItem("hito-log-level", "debug"),
  );
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    phase = "copies";
    await page.goto("/settings?section=plugins");
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeVisible();
    await page.getByText("Services for this server", { exact: true }).click();
    const address = page.getByLabel("Service address", { exact: true });
    await expect(page.getByLabel("Service name", { exact: true })).toHaveCount(
      0,
    );
    await expect(
      page.getByLabel("Custom label (optional)", { exact: true }),
    ).toBeHidden();
    await address.fill(fixture.address);
    const offered = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/plugins/services" &&
        response.request().method() === "POST",
    );
    await page
      .getByRole("button", { name: "Offer service", exact: true })
      .click();
    const offerResponse = await offered;
    expect(offerResponse.status()).toBe(201);
    expect(offerResponse.request().postDataJSON()).toEqual({
      base_url: fixture.address,
    });
    service = await offerResponse.json();
    if (!service)
      throw new Error("Offering an address must return the reviewed service");
    expect(service.name).toBe(fixture.manifest.name);
    const selector = page.getByRole("combobox", {
      name: "Service",
      exact: true,
    });
    await chooseOption(selector, fixture.manifest.name);
    await expect(selector).toContainText(fixture.manifest.name);
    const token = page.getByLabel("Bearer token (optional)", { exact: true });
    const reviewButton = page.getByRole("button", {
      name: "Review connection",
      exact: true,
    });
    const targets = await layouts(page, info, "provider-artwork-connect", [
      address,
      selector,
      token,
      reviewButton,
    ]);
    await token.fill(fixture.token);
    await token.focus();
    await token.press("Tab");
    await expect(reviewButton).toBeFocused();
    const installationReview = await reviewPluginInstallation(
      page,
      service,
      fixture.manifest.name,
      () => reviewButton.press("Enter"),
    );
    const installed = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/plugins" &&
        response.request().method() === "POST",
    );
    await page
      .getByRole("button", { name: "Accept and connect", exact: true })
      .click();
    const installResponse = await installed;
    expect(installResponse.status()).toBe(201);
    expect(
      isDeepStrictEqual(installResponse.request().postDataJSON(), {
        ...installationReview.sent,
        accept_manifest_digest: installationReview.body.digest,
      }),
    ).toBe(true);
    connection = await installResponse.json();
    if (!connection)
      throw new Error("Connecting the provider must return its installation");
    expect(connection.manifest.capabilities).toEqual(["playlists"]);
    const connected = page.locator(".plugin-connection").filter({
      has: page.getByRole("heading", {
        name: fixture.manifest.name,
        exact: true,
      }),
    });
    await expect(
      connected.getByRole("button", {
        name: "Check connection",
        exact: true,
      }),
    ).toBeEnabled();
    await expect(
      connected.getByText("Token saved", { exact: true }),
    ).toBeVisible();
    if (await token.count()) await expect(token).toHaveValue("");
    const credentialState = await page.evaluate(() =>
      JSON.stringify({
        inputs: Array.from(
          document.querySelectorAll("input"),
          (input) => input.value,
        ),
        local: Object.entries(localStorage),
        session: Object.entries(sessionStorage),
      }),
    );
    expect(credentialState).not.toContain(fixture.token);
    const connectionId = connection.id;
    const importPath = `/api/v1/plugins/${connectionId}/playlists/import`;
    const initial = await get<Schema<"Playlist">[]>(page, "/api/v1/playlists");
    for (let index = 0; index < 2; index++) {
      await page.goto("/playlists");
      const form = page.getByRole("region", { name: "Import a playlist" });
      await form
        .getByRole("textbox", { name: "Playlist link or identifier" })
        .fill(fixture.reference);
      const imported = page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === importPath &&
          response.request().method() === "POST",
      );
      const checked = page.waitForResponse(
        (response) =>
          /^\/api\/v1\/recordings\/[^/]+\/artwork\/availability$/.test(
            new URL(response.url()).pathname,
          ) && response.request().method() === "GET",
      );
      await form
        .getByRole("button", { name: "Import a copy", exact: true })
        .click();
      const response = await imported;
      expect(response.status()).toBe(200);
      const receipt: Schema<"PlaylistImportReceipt"> = await response.json();
      copies.push(receipt.playlist_id);
      await expect(page).toHaveURL(
        new URL(`/playlists/${receipt.playlist_id}`, baseURL).href,
      );
      await expect(
        page.getByRole("heading", { name: fixture.detail.name, exact: true }),
      ).toBeVisible();
      const detail = await get<Schema<"PlaylistDetail">>(
        page,
        `/api/v1/playlists/${receipt.playlist_id}`,
      );
      expect(detail.entries).toHaveLength(2);
      const id = detail.entries[0].recording_id;
      if (!id)
        throw new Error(
          "Copied metadata needs its durable Hito recording identity",
        );
      if (recording) expect(id).toBe(recording);
      recording = id;
      expect(
        detail.entries.map((entry) => [
          entry.position,
          entry.recording_id,
          entry.origin?.occurrence_id,
        ]),
      ).toEqual([
        [0, id, "first"],
        [1, id, "duplicate"],
      ]);
      expect(detail.entries.every((entry) => !entry.available)).toBe(true);
      const checkedResponse = await checked;
      expect(checkedResponse.status()).toBe(200);
      expect(new URL(checkedResponse.url()).pathname).toBe(
        `/api/v1/recordings/${id}/artwork/availability`,
      );
      const value: Schema<"ArtworkAvailability"> = await checkedResponse.json();
      expect(value).toEqual({ can_fetch: false });
      availability.push({ phase: "copies", value });
    }
    if (!recording) throw new Error("Both copies must share a saved recording");
    const recordingId = recording;
    const artworkPath = `/api/v1/recordings/${recordingId}/artwork`;
    await expect(
      page
        .getByRole("table", { name: "Playlist entries" })
        .locator(".artwork img"),
    ).toHaveCount(0);
    const documentToken = randomUUID();
    await page.evaluate((token) => {
      document.documentElement.dataset.artworkApprovalDocument = token;
    }, documentToken);
    expect(copies[0]).not.toBe(copies[1]);
    expect(imports).toHaveLength(2);
    expect(imports[0].request_id).not.toBe(imports[1].request_id);
    for (const body of imports)
      expect(body).toEqual({
        request_id: expect.any(String),
        installation_revision: connection.revision,
        playlist_reference: fixture.reference,
      });
    await expect
      .poll(() =>
        availability.some(
          (item) => item.phase === "copies" && !item.value.can_fetch,
        ),
      )
      .toBe(true);
    expect(jobs).toEqual([]);
    expect(artworkEnqueues).toEqual([]);
    expect(
      fixture.requests.filter((request) => request.path.endsWith("/artwork")),
    ).toEqual([]);
    const title = `${fixture.prefix} personal title`;
    const artist = `${fixture.prefix} personal artist`;
    const before = await savePersonalState(
      page,
      copies,
      recordingId,
      title,
      artist,
      initial,
      headers,
    );
    phase = "approval";
    fixture.enableArtwork();
    await page
      .getByRole("link", { name: "Account and settings", exact: true })
      .click();
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Plugins", exact: true })
      .click();
    await expect(page.locator("html")).toHaveAttribute(
      "data-artwork-approval-document",
      documentToken,
    );
    const plugin = page.getByRole("listitem").filter({
      has: page.getByRole("heading", {
        name: fixture.manifest.name,
        exact: true,
      }),
    });
    const refreshPath = `/api/v1/plugins/${connectionId}/refresh`;
    const reviewed = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === refreshPath &&
        response.request().method() === "POST",
    );
    await plugin
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    const reviewResponse = await reviewed;
    expect(reviewResponse.status()).toBe(200);
    const review: Schema<"PluginRefresh"> = await reviewResponse.json();
    expect(review.kind).toBe("review_required");
    await expect(
      plugin.getByRole("heading", {
        name: "Review plugin changes",
        exact: true,
      }),
    ).toBeVisible();
    expect(jobs).toEqual([]);
    expect(artworkEnqueues).toEqual([]);
    const accept = plugin.getByRole("button", {
      name: "Accept changes",
      exact: true,
    });
    targets.push(
      ...(await layouts(page, info, "provider-artwork-review", [accept])),
    );
    await accept.focus();
    await expect(accept).toBeFocused();
    const accepted = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === refreshPath &&
        response.request().method() === "POST",
    );
    await accept.press("Enter");
    const acceptedResponse = await accepted;
    expect(acceptedResponse.status()).toBe(200);
    const refresh: Schema<"PluginRefresh"> = await acceptedResponse.json();
    if (refresh.kind !== "updated")
      throw new Error(
        "Accepted artwork capability must update this connection",
      );
    connection = refresh.installation;
    expect(connection.manifest.capabilities).toContain("artwork");
    await expect(
      plugin.getByRole("button", { name: "Check connection", exact: true }),
    ).toBeFocused();
    expect(
      fixture.requests.filter((request) => request.path.endsWith("/artwork")),
    ).toEqual([]);
    expect(await savedState(page, copies, recordingId)).toEqual(before);
    phase = "retrieving";
    const enqueued = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === `${artworkPath}/fetch` &&
        response.request().method() === "POST",
    );
    const eligible = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === `${artworkPath}/availability` &&
        response.request().method() === "GET",
    );
    await page
      .locator(".sidebar-playlists")
      .locator(`a[href="/playlists/${copies[0]}"]`)
      .click();
    await expect(page).toHaveURL(
      new URL(`/playlists/${copies[0]}`, baseURL).href,
    );
    await expect(page.locator("html")).toHaveAttribute(
      "data-artwork-approval-document",
      documentToken,
    );
    const eligibleResponse = await eligible;
    expect(eligibleResponse.status()).toBe(200);
    const eligibility: Schema<"ArtworkAvailability"> =
      await eligibleResponse.json();
    expect(eligibility).toEqual({ can_fetch: true });
    availability.push({ phase: "retrieving", value: eligibility });
    const enqueueResponse = await enqueued;
    expect(enqueueResponse.status()).toBe(202);
    expect(enqueueResponse.request().postData()).toBeNull();
    const receipt: Schema<"ArtworkFetch"> = await enqueueResponse.json();
    expect(receipt.recording_id).toBe(recordingId);
    expect(["queued", "running"]).toContain(receipt.status);
    jobs.push({
      phase: "retrieving",
      method: "POST",
      path: `${artworkPath}/fetch`,
      value: receipt,
    });
    await expect.poll(() => fixture.heldImages).toBe(1);
    const statusPath = `/api/v1/artwork-fetches/${receipt.id}`;
    const runningResponse = await page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === statusPath &&
        response.request().method() === "GET",
    );
    expect(runningResponse.status()).toBe(200);
    const running: Schema<"ArtworkFetch"> = await runningResponse.json();
    expect(running).toMatchObject({
      id: receipt.id,
      recording_id: recordingId,
      status: "running",
    });
    jobs.push({
      phase: "retrieving",
      method: "GET",
      path: statusPath,
      value: running,
    });
    await expect
      .poll(() =>
        jobs.some(
          (item) =>
            item.value.id === receipt.id && item.value.status === "running",
        ),
      )
      .toBe(true);
    expect(
      availability.some(
        (item) => item.phase === "retrieving" && item.value.can_fetch,
      ),
    ).toBe(true);
    expect(await savedState(page, copies, recordingId)).toEqual(before);
    const providerArtwork = fixture.requests.filter(
      (request) =>
        request.path === "/fixture/v1/artwork" &&
        (request.body as { entity?: string } | null)?.entity === "recording",
    );
    expect(providerArtwork).toHaveLength(1);
    expect(providerArtwork[0]).toMatchObject({
      method: "POST",
      body: { entity: "recording", provider_id: fixture.recording.id },
    });
    fixture.releaseImage();
    let ready = false;
    while (!ready) {
      const statusResponse = await page.waitForResponse(
        (response) =>
          new URL(response.url()).pathname === statusPath &&
          response.request().method() === "GET",
      );
      expect(statusResponse.status()).toBe(200);
      const status: Schema<"ArtworkFetch"> = await statusResponse.json();
      expect(status).toMatchObject({
        id: receipt.id,
        recording_id: recordingId,
      });
      expect(["queued", "running", "ready"]).toContain(status.status);
      jobs.push({
        phase: "retrieving",
        method: "GET",
        path: statusPath,
        value: status,
      });
      ready = status.status === "ready";
    }
    await expect
      .poll(() =>
        jobs.some(
          (item) =>
            item.value.id === receipt.id && item.value.status === "ready",
        ),
      )
      .toBe(true);
    const backfilledRows = page
      .getByRole("table", { name: "Playlist entries" })
      .locator("tbody")
      .getByRole("row");
    await expect(backfilledRows).toHaveCount(2);
    for (const row of await backfilledRows.all()) {
      await expect
        .poll(() =>
          row
            .locator(".artwork img")
            .evaluate(
              (element: HTMLImageElement) =>
                element.complete && element.naturalWidth > 0,
            ),
        )
        .toBe(true);
      await expect(row.locator(".artwork img")).toHaveAttribute(
        "src",
        artworkPath,
      );
    }
    phase = "ready";
    await loadedCopies(page, copies, recordingId, title, baseURL);
    for (const copy of copies) {
      await page.goto(`/playlists/${copy}`);
      const cover = page.locator(".tinted-header .artwork img");
      await expect(cover).toHaveAttribute(
        "src",
        `/api/v1/playlists/${copy}/artwork`,
      );
      await expect
        .poll(() =>
          cover.evaluate(
            (element: HTMLImageElement) =>
              element.complete && element.naturalWidth > 0,
          ),
        )
        .toBe(true);
    }
    expect(jobs.filter((item) => item.method === "POST")).toHaveLength(1);
    expect(artworkEnqueues).toEqual([
      { phase: "retrieving", path: `${artworkPath}/fetch` },
    ]);
    expect(imports).toHaveLength(2);
    expect(await savedState(page, copies, recordingId)).toEqual(before);
    const retained = await page.request.get(artworkPath);
    expect(retained.status()).toBe(200);
    expect(retained.headers()["cache-control"]).toBe("private, no-store");
    expect(retained.headers()["content-type"]).toBe("image/png");
    expect(await retained.body()).toEqual(fixture.image);
    const rows = page
      .getByRole("table", { name: "Playlist entries" })
      .locator("tbody")
      .getByRole("row");
    targets.push(
      ...(await layouts(page, info, "provider-artwork-retained", [
        rows
          .first()
          .getByRole("button", { name: `Unlike ${title}`, exact: true }),
        rows
          .first()
          .getByRole("button", { name: `Actions for ${title}`, exact: true }),
      ])),
    );
    await page.goto("/settings?section=plugins");
    await plugin.getByRole("button", { name: "Remove", exact: true }).click();
    const remove = plugin.getByRole("button", {
      name: "Remove plugin",
      exact: true,
    });
    await remove.focus();
    await expect(remove).toBeFocused();
    const removed = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/plugins/${connectionId}` &&
        response.request().method() === "DELETE",
    );
    await remove.press("Enter");
    expect((await removed).status()).toBe(204);
    connection = null;
    phase = "disconnected";
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeFocused();
    const callsAfterRemoval = fixture.requests.length;
    fixture.offline();
    await loadedCopies(page, copies, recordingId, title, baseURL);
    await page.reload();
    await expect
      .poll(() =>
        page
          .locator(".tinted-header .artwork img")
          .evaluate(
            (element: HTMLImageElement) =>
              element.complete && element.naturalWidth > 0,
          ),
      )
      .toBe(true);
    expect(await savedState(page, copies, recordingId)).toEqual(before);
    expect(fixture.requests).toHaveLength(callsAfterRemoval);
    expect(jobs.filter((item) => item.method === "POST")).toHaveLength(1);
    expect(artworkEnqueues).toHaveLength(1);
    const username = `artwork-${randomUUID().slice(0, 8)}`;
    const password = "other artwork account password";
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
      observe(listener, "other");
      await listener.addInitScript(() =>
        localStorage.setItem("hito-log-level", "debug"),
      );
      phase = "sign-in";
      await signIn(listener, username, password);
      phase = "disconnected";
      for (const path of [
        artworkPath,
        `${artworkPath}/availability`,
        `/api/v1/artwork-fetches/${receipt.id}`,
      ])
        expect((await listener.request.get(path)).status()).toBe(404);
      expect(
        (await get<Schema<"PluginSettings">>(listener, "/api/v1/plugins"))
          .installations,
      ).toEqual([]);
      const otherStorage = await listener.evaluate(() =>
        JSON.stringify({
          local: Object.entries(localStorage),
          session: Object.entries(sessionStorage),
        }),
      );
      for (const privateValue of [
        fixture.token,
        fixture.recording.id,
        fixture.reference,
        title,
        fixture.recording.title,
      ])
        expect(otherStorage).not.toContain(privateValue);
    } finally {
      await other.close();
    }
    expect(fixture.requests).toHaveLength(callsAfterRemoval);
    for (const [index, request] of fixture.requests.entries()) {
      expect(request.authorization).toBe(
        index === 0 ? undefined : `Bearer ${fixture.token}`,
      );
      expect(request.cookie).toBeUndefined();
      expect(request.protocol).toBe("1");
      expect([
        "/fixture/v1/manifest",
        "/fixture/v1/playlists/detail",
        "/fixture/v1/playlists/items",
        "/fixture/v1/artwork",
      ]).toContain(request.path);
    }
    await diagnostics.assertPrivacy(page, fixture.address, [
      fixture.token,
      fixture.reference,
      fixture.address,
      `${fixture.address}v1/artwork`,
      fixture.recording.id,
      fixture.detail.id,
      title,
      artist,
      fixture.recording.title,
    ]);
    diagnostics.assertFailures(artworkPath, baseURL, copies);
    await info.attach("provider-artwork-evidence", {
      body: JSON.stringify(
        {
          copies,
          recording: recordingId,
          entries: before.copies.map((copy) =>
            copy.entries.map((entry) => entry.entry_id),
          ),
          imports: imports.length,
          availability,
          jobs,
          artworkEnqueues,
          providerArtworkRequests: providerArtwork.length,
          retainedBytes: fixture.image.length,
          targets,
        },
        null,
        2,
      ),
      contentType: "application/json",
    });
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
            device_id: "provider-artwork-e2e-cleanup",
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
