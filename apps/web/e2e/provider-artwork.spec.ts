import { randomUUID } from "node:crypto";
import {
  expect,
  test,
  type Locator,
  type Page,
  type TestInfo,
} from "@playwright/test";
import { artworkFixture } from "./fixtures/artwork";
import type { components } from "../src/generated";

type Schema = components["schemas"];
type SavedState = {
  playlists: Schema["Playlist"][];
  copies: Schema["PlaylistDetail"][];
  catalog: Schema["RecordingPage"];
  metadata: Schema["RecordingMetadata"];
  activity: Schema["Activity"];
  feed: Schema["SyncPage"];
  sources: Schema["Source"][];
};
type Phase =
  | "sign-in"
  | "copies"
  | "approval"
  | "retrieving"
  | "ready"
  | "disconnected";

async function signIn(
  page: Page,
  username = "admin",
  password = "docker browser test password",
) {
  await page.goto("/playlists");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  const setup = await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .count();
  await page
    .getByRole("button", {
      name: setup ? "Create administrator" : "Sign in",
      exact: true,
    })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

async function get<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

async function savedState(
  page: Page,
  copies: string[],
  recording: string,
): Promise<SavedState> {
  return {
    playlists: await get(page, "/api/v1/playlists"),
    copies: await Promise.all(
      copies.map((id) =>
        get<Schema["PlaylistDetail"]>(page, `/api/v1/playlists/${id}`),
      ),
    ),
    catalog: await get(page, "/api/v1/recordings"),
    metadata: await get(page, `/api/v1/recordings/${recording}/metadata`),
    activity: await get(page, "/api/v1/activity"),
    feed: await get(page, "/api/v1/sync?cursor=0"),
    sources: await get(page, `/api/v1/recordings/${recording}/sources`),
  };
}

async function layouts(
  page: Page,
  info: TestInfo,
  stage: string,
  controls: Locator[],
) {
  const measured: {
    viewport: number;
    label: string;
    width: number;
    height: number;
  }[] = [];
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
      if (!bounds || !dock)
        throw new Error("The control and player need visible bounds");
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width);
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(dock.y);
      if (
        await control.evaluate((element) =>
          element.matches("input, select, .track-actions .icon-button"),
        )
      ) {
        expect(bounds.width).toBeGreaterThanOrEqual(44);
        expect(bounds.height).toBeGreaterThanOrEqual(44);
      }
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
      measured.push({
        viewport: viewport.width,
        label:
          (await control.getAttribute("aria-label")) ??
          (await control.innerText()),
        width: bounds.width,
        height: bounds.height,
      });
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
  return measured;
}

async function loadedCopies(
  page: Page,
  copies: string[],
  recording: string,
  title: string,
  baseURL: string,
) {
  for (const copy of copies) {
    await page.goto(`/playlists/${copy}`);
    const rows = page
      .getByRole("table", { name: "Playlist entries" })
      .locator("tbody")
      .getByRole("row");
    await expect(rows).toHaveCount(2);
    await expect(rows.locator(".track-title")).toHaveText([title, title]);
    await expect(
      page.getByRole("button", { name: "Play playlist", exact: true }),
    ).toBeDisabled();
    for (const row of await rows.all()) {
      await expect(
        row.getByRole("button", { name: `Play ${title}`, exact: true }),
      ).toBeDisabled();
      const image = row.locator(".artwork img");
      await expect(image).toHaveAttribute(
        "src",
        `/api/v1/recordings/${recording}/artwork`,
      );
      await expect
        .poll(() =>
          image.evaluate(
            (element: HTMLImageElement) =>
              element.complete && element.naturalWidth > 0,
          ),
        )
        .toBe(true);
    }
    for (const image of await page.locator("img").all()) {
      const url = await image.evaluate(
        (element: HTMLImageElement) => element.currentSrc || element.src,
      );
      expect(new URL(url).origin).toBe(new URL(baseURL).origin);
    }
    await expect(page.locator("audio")).not.toHaveAttribute("src");
  }
}

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
  const imports: Schema["ImportProviderPlaylist"][] = [];
  const availability: { phase: Phase; value: Schema["ArtworkAvailability"] }[] =
    [];
  const jobs: {
    phase: Phase;
    method: string;
    path: string;
    value: Schema["ArtworkFetch"];
  }[] = [];
  const artworkEnqueues: { phase: Phase; path: string }[] = [];
  const logs: Promise<string>[] = [];
  const urls: string[] = [];
  const imageURLs: string[] = [];
  const errors: string[] = [];
  const consoleErrors: { text: string; url: string }[] = [];
  const failedResponses: {
    owner: string;
    url: string;
    method: string;
    status: number;
    phase: Phase;
  }[] = [];
  let phase: Phase = "sign-in";
  let service: Schema["PluginService"] | null = null;
  let connection: Schema["PluginInstallation"] | null = null;
  let recording: string | undefined;
  const observe = (observed: Page, owner: string) => {
    observed.on("pageerror", (error) => errors.push(error.message));
    observed.on("console", (message) => {
      if (message.type() === "error")
        consoleErrors.push({
          text: message.text(),
          url: message.location().url,
        });
      logs.push(
        Promise.all(message.args().map((argument) => argument.jsonValue()))
          .then((values) => JSON.stringify(values))
          .catch(() => message.text()),
      );
    });
    observed.on("request", (request) => {
      urls.push(request.url());
      if (request.resourceType() === "image") imageURLs.push(request.url());
      if (
        request.method() === "POST" &&
        /\/plugins\/[^/]+\/playlists\/import$/.test(
          new URL(request.url()).pathname,
        )
      )
        imports.push(request.postDataJSON());
      if (
        request.method() === "POST" &&
        new URL(request.url()).pathname.endsWith("/artwork/fetch")
      )
        artworkEnqueues.push({
          phase,
          path: new URL(request.url()).pathname,
        });
    });
    observed.on("response", (response) => {
      const url = new URL(response.url());
      if (url.origin !== origin) return;
      const at = phase;
      if (response.status() >= 400)
        failedResponses.push({
          owner,
          url: response.url(),
          method: response.request().method(),
          status: response.status(),
          phase: at,
        });
    });
  };
  observe(page, "owner");
  await page.addInitScript(() =>
    localStorage.setItem("hito-log-level", "debug"),
  );
  try {
    await signIn(page);
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
    await selector.selectOption(service.id);
    await expect(selector.locator("option:checked")).toHaveText(
      fixture.manifest.name,
    );
    const token = page.getByLabel("Bearer token (optional)", { exact: true });
    const connect = page.getByRole("button", {
      name: "Connect service",
      exact: true,
    });
    const targets = await layouts(page, info, "provider-artwork-connect", [
      address,
      selector,
      token,
      connect,
    ]);
    await token.fill(fixture.token);
    await token.focus();
    await token.press("Tab");
    await expect(connect).toBeFocused();
    const installed = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/plugins" &&
        response.request().method() === "POST",
    );
    await connect.press("Enter");
    const installResponse = await installed;
    expect(installResponse.status()).toBe(201);
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
    const initial = await get<Schema["Playlist"][]>(page, "/api/v1/playlists");
    for (let index = 0; index < 2; index++) {
      await page.goto("/playlists");
      const form = page.getByRole("region", { name: "Import a playlist" });
      await form
        .getByRole("combobox", { name: "Playlist service" })
        .selectOption(connectionId);
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
      const receipt: Schema["PlaylistImportReceipt"] = await response.json();
      copies.push(receipt.playlist_id);
      await expect(page).toHaveURL(
        new URL(`/playlists/${receipt.playlist_id}`, baseURL).href,
      );
      await expect(
        page.getByRole("heading", { name: fixture.detail.name, exact: true }),
      ).toBeVisible();
      const detail = await get<Schema["PlaylistDetail"]>(
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
      const value: Schema["ArtworkAvailability"] = await checkedResponse.json();
      expect(value).toEqual({ can_fetch: false });
      availability.push({ phase: "copies", value });
    }
    if (!recording) throw new Error("Both copies must share a saved recording");
    const recordingId = recording;
    const artworkPath = `/api/v1/recordings/${recordingId}/artwork`;
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
    const edited = await page.request.post("/api/v1/sync", {
      headers,
      data: {
        device_id: "provider-artwork-e2e",
        operations: [
          {
            id: randomUUID(),
            action: {
              kind: "recording_title_set",
              recording_id: recordingId,
              title,
            },
          },
          {
            id: randomUUID(),
            action: {
              kind: "recording_artist_set",
              recording_id: recordingId,
              artist,
            },
          },
          {
            id: randomUUID(),
            action: {
              kind: "like_set",
              recording_id: recordingId,
              liked: true,
            },
          },
          {
            id: randomUUID(),
            action: {
              kind: "progress_set",
              recording_id: recordingId,
              position_ms: 4200,
              speed: 1.25,
            },
          },
        ],
      } satisfies Schema["SyncBatch"],
    });
    expect(edited.status()).toBe(200);
    const before = await savedState(page, copies, recordingId);
    expect(before.playlists).toHaveLength(initial.length + 2);
    expect(
      new Set(
        before.copies.flatMap((copy) =>
          copy.entries.map((entry) => entry.entry_id),
        ),
      ).size,
    ).toBe(4);
    expect(before.metadata.title.personal).toBe(title);
    expect(before.metadata.artist.personal).toBe(artist);
    expect(
      before.catalog.items.find((item) => item.id === recordingId),
    ).toMatchObject({ title, artist, liked: true, available: false });
    expect(
      before.activity.history.find((item) => item.id === recordingId),
    ).toMatchObject({ liked: true, position_ms: 4200, speed: 1.25 });
    expect(before.sources).toEqual([]);
    phase = "approval";
    fixture.enableArtwork();
    await page.goto("/settings?section=plugins");
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
    const review: Schema["PluginRefresh"] = await reviewResponse.json();
    expect(review.kind).toBe("review_required");
    await expect(
      plugin.getByRole("heading", {
        name: "Review plugin changes",
        exact: true,
      }),
    ).toBeVisible();
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
    const refresh: Schema["PluginRefresh"] = await acceptedResponse.json();
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
    await page.goto(`/playlists/${copies[0]}`);
    const eligibleResponse = await eligible;
    expect(eligibleResponse.status()).toBe(200);
    const eligibility: Schema["ArtworkAvailability"] =
      await eligibleResponse.json();
    expect(eligibility).toEqual({ can_fetch: true });
    availability.push({ phase: "retrieving", value: eligibility });
    const enqueueResponse = await enqueued;
    expect(enqueueResponse.status()).toBe(202);
    expect(enqueueResponse.request().postData()).toBeNull();
    const receipt: Schema["ArtworkFetch"] = await enqueueResponse.json();
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
    const running: Schema["ArtworkFetch"] = await runningResponse.json();
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
      (request) => request.path === "/fixture/v1/artwork",
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
      const status: Schema["ArtworkFetch"] = await statusResponse.json();
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
          .getByRole("link", { name: `Details for ${title}`, exact: true }),
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
          .locator(".collection-hero .artwork img")
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
    const other = await browser.newContext({ baseURL });
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
        (await get<Schema["PluginSettings"]>(listener, "/api/v1/plugins"))
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
    expect(
      urls.filter(
        (url) => new URL(url).origin === new URL(fixture.address).origin,
      ),
    ).toEqual([]);
    expect(imageURLs.length).toBeGreaterThan(0);
    for (const url of imageURLs) expect(new URL(url).origin).toBe(origin);
    expect(urls.some((url) => /\/audio(?:\?|$)/.test(url))).toBe(false);
    const storage = await page.evaluate(() =>
      JSON.stringify({
        local: Object.entries(localStorage),
        session: Object.entries(sessionStorage),
      }),
    );
    const browserLogs = (await Promise.all(logs)).join("\n");
    for (const value of [
      fixture.token,
      fixture.reference,
      fixture.address,
      `${fixture.address}v1/artwork`,
      fixture.recording.id,
      fixture.detail.id,
      title,
      artist,
      fixture.recording.title,
    ]) {
      expect(browserLogs).not.toContain(value);
      expect(storage).not.toContain(value);
      for (const url of urls) {
        expect(url).not.toContain(value);
        expect(url).not.toContain(encodeURIComponent(value));
      }
    }
    expect(errors).toEqual([]);
    const messages = new Map([
      [
        401,
        "Failed to load resource: the server responded with a status of 401 (Unauthorized)",
      ],
      [
        404,
        "Failed to load resource: the server responded with a status of 404 (Not Found)",
      ],
    ]);
    for (const response of failedResponses) {
      expect(response.method).toBe("GET");
      if (response.url === new URL("/api/v1/me", baseURL).href) {
        expect(response.status).toBe(401);
        expect(response.phase).toBe("sign-in");
      } else {
        expect(response.owner).toBe("owner");
        expect(response.url).toBe(new URL(artworkPath, baseURL).href);
        expect(response.status).toBe(404);
        expect(["copies", "retrieving"]).toContain(response.phase);
      }
    }
    for (const owner of ["owner", "other"])
      expect(
        failedResponses.filter(
          (response) => response.owner === owner && response.status === 401,
        ),
      ).toHaveLength(1);
    expect(
      failedResponses.some(
        (response) =>
          response.phase === "retrieving" && response.status === 404,
      ),
    ).toBe(true);
    const correlated = new Set<number>();
    const unexpected = consoleErrors.filter((diagnostic) => {
      const index = failedResponses.findIndex(
        (response, index) =>
          !correlated.has(index) &&
          response.url === diagnostic.url &&
          diagnostic.text === messages.get(response.status),
      );
      if (index < 0) return true;
      correlated.add(index);
      return false;
    });
    expect(unexpected).toEqual([]);
    expect(correlated.size).toBe(failedResponses.length);
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
          } satisfies Schema["SyncBatch"],
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
