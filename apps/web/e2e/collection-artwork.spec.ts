import { readFile, writeFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { savedState, signIn } from "./fixtures/artwork-browser-checks";
import {
  action,
  decoded,
  downloadAbsence,
  hash,
  importCopy,
  installProvider,
  json,
  preservePersonalEdits,
  approveArtwork,
  retained,
} from "./fixtures/collection-artwork-actions";
import {
  collectionArtworkProvider,
  ownedOriginals,
} from "./fixtures/collection-artwork-provider";
import {
  collectionArtworkEvidence,
  type Receipt,
} from "./fixtures/collection-artwork-evidence";
import {
  privateAccountDenials,
  revokedPublication,
  loggedOutPublication,
} from "./fixtures/collection-artwork-guards";
import {
  coalescedRendererRetry,
  lateRendererRetry,
  retainedRetry,
} from "./fixtures/collection-artwork-retry";

type Schema = components["schemas"];

test("packaged albums and independent imported copies retain exact private covers across retry and revocation", async ({
  page,
  browser,
  baseURL,
}, info) => {
  if (!baseURL)
    throw new Error("An isolated packaged artwork server is required");
  const original = await ownedOriginals();
  expect(original[0].bytes.length).toBe(32044);
  expect(original[0].bytes.subarray(0, 4).toString()).toBe("RIFF");
  const fixture = await collectionArtworkProvider();
  expect(new Set(Object.values(fixture.covers).map(hash)).size).toBe(3);
  const evidence = collectionArtworkEvidence(new URL(baseURL).origin);
  await evidence.observe(page.context());
  try {
    await signIn(page);
    await evidence.captureSession(page.context());
    const owner = await json<Schema["User"]>(page, "/api/v1/me");
    const api = await page.request.get("/api/v1/openapi.json", {
      timeout: 10_000,
    });
    expect(api.status()).toBe(200);
    expect(await api.json()).toEqual(
      JSON.parse(await readFile("/workspace/docs/openapi.json", "utf8")),
    );
    await api.dispose();
    for (const [asset, expected] of [
      ["app.js", process.env.HITO_ARTWORK_JS_SHA256],
      ["app.css", process.env.HITO_ARTWORK_CSS_SHA256],
    ]) {
      expect(expected).toMatch(/^[0-9a-f]{64}$/);
      const response = await page.request.get(`/assets/${asset}`, {
        timeout: 10_000,
      });
      expect(response.status()).toBe(200);
      expect(hash(await response.body())).toBe(expected);
      await response.dispose();
    }
    const connected = await installProvider(page, fixture);
    let installation = connected.installation;
    const copies = [];
    for (let count = 0; count < 2; count++)
      copies.push(await importCopy(page, fixture, installation));
    const copyIds = copies.map((copy) => copy.playlist.id);
    expect(new Set(copyIds).size).toBe(2);
    expect(
      new Set(
        copies.flatMap((copy) => copy.entries.map((entry) => entry.entry_id)),
      ).size,
    ).toBe(4);
    const recording = copies[0].entries[0].recording_id;
    if (!recording)
      throw new Error("The copied recording must have a durable identity");
    expect(
      copies[1].entries.every((entry) => entry.recording_id === recording),
    ).toBe(true);
    const catalog = await json<Schema["RecordingPage"]>(
      page,
      "/api/v1/recordings",
    );
    expect(catalog.total).toBe(1);
    const release = catalog.items[0].release_id;
    if (!release)
      throw new Error("The exact provider album witness must be imported");
    const downloadsBefore = await downloadAbsence(
      page,
      owner.id,
      [recording],
      copyIds,
    );
    expect(downloadsBefore).toEqual({
      recordings: 1,
      playlists: 2,
      reads: 3,
      status: 404,
    });
    const titles = await preservePersonalEdits(
      page,
      copyIds,
      recording,
      release,
      fixture.prefix,
    );
    const saved = async () => ({
      ...(await savedState(page, copyIds, recording)),
      albums: await json<Schema["ReleasePage"]>(page, "/api/v1/releases"),
      albumMetadata: await json<Schema["ReleaseMetadata"]>(
        page,
        `/api/v1/releases/${release}/metadata`,
      ),
    });
    const before = await saved();
    expect(before.metadata.title.personal).toBe(titles.track);
    expect(
      before.activity.history.find((item) => item.id === recording),
    ).toMatchObject({ liked: true, position_ms: 4200, speed: 1.25 });
    expect(before.copies.map((copy) => copy.playlist.name)).toEqual(
      titles.copies,
    );
    expect(before.sources).toEqual([]);
    await evidence.settle();
    expect(evidence.enqueues).toEqual([]);
    expect(
      fixture.requests.filter((request) => request.path.endsWith("/artwork")),
    ).toEqual([]);
    const approval = await approveArtwork(page, fixture, installation);
    expect(
      fixture.requests.filter((request) => request.path.endsWith("/artwork")),
    ).toEqual([]);
    const accepted = await action<Schema["PluginRefresh"]>(
      page,
      approval.path,
      200,
      approval.card.getByRole("button", {
        name: "Accept changes",
        exact: true,
      }),
    );
    if (accepted.kind !== "updated")
      throw new Error("Artwork must be explicitly approved");
    installation = accepted.installation;
    expect(installation.manifest.capabilities).toEqual([
      "playlists",
      "artwork",
    ]);
    evidence.phase("retrieving");
    const paths = {
      recording: `/api/v1/recordings/${recording}/artwork`,
      release: `/api/v1/releases/${release}/artwork`,
      copies: copyIds.map((id) => `/api/v1/playlists/${id}/artwork`),
    };
    const hashes: string[] = [];
    for (const [index, id] of copyIds.entries()) {
      await page.goto(`/playlists/${id}`);
      await expect(
        page.getByRole("heading", { name: titles.copies[index], exact: true }),
      ).toBeVisible();
      await decoded(
        page.locator(".collection-hero .artwork img"),
        paths.copies[index],
      );
      await expect(
        page
          .getByRole("table", { name: "Playlist entries" })
          .locator("tbody tr"),
      ).toHaveCount(2);
      await decoded(
        page
          .getByRole("table", { name: "Playlist entries" })
          .locator(".artwork img")
          .first(),
        paths.recording,
      );
      await expect
        .poll(() =>
          evidence.enqueues
            .filter((receipt) =>
              [id, recording, release].includes(receipt.target),
            )
            .every((receipt) => evidence.readyJobs().includes(receipt.id)),
        )
        .toBe(true);
      hashes.push(
        await retained(page, paths.copies[index], fixture.covers.playlist),
      );
    }
    await page
      .getByRole("navigation", { name: "Main navigation" })
      .getByRole("link", { name: "Albums", exact: true })
      .click();
    await expect
      .poll(() =>
        evidence.enqueues.some(
          (receipt) =>
            receipt.kind === "release" &&
            receipt.target === release &&
            evidence.readyJobs().includes(receipt.id),
        ),
      )
      .toBe(true);
    await page
      .locator(".album-card")
      .getByRole("link", { name: titles.release, exact: true })
      .click();
    await expect(page).toHaveURL(new RegExp(`/albums/${release}$`));
    await decoded(page.locator(".album-hero .artwork img"), paths.release);
    hashes.push(await retained(page, paths.release, fixture.covers.release));
    await page.goto(`/recordings/${recording}`);
    await decoded(page.locator(".metadata-hero .artwork img"), paths.recording);
    hashes.push(
      await retained(page, paths.recording, fixture.covers.recording),
    );
    await page.goto("/playlists");
    for (const [index, id] of copyIds.entries())
      await decoded(
        page.locator(`.playlist-list a[href="/playlists/${id}"] .artwork img`),
        paths.copies[index],
      );
    await evidence.settle();
    const acceptedReceipts: Receipt[] = evidence.enqueues.filter(
      (receipt) =>
        receipt.target === recording ||
        receipt.target === release ||
        copyIds.includes(receipt.target),
    );
    const ready = [
      ...new Map(
        acceptedReceipts.map((receipt) => [receipt.id, receipt]),
      ).values(),
    ];
    expect(acceptedReceipts.length).toBeGreaterThanOrEqual(4);
    expect(acceptedReceipts.length).toBeLessThanOrEqual(8);
    expect(ready).toHaveLength(4);
    for (const target of [recording, release, ...copyIds]) {
      expect(
        acceptedReceipts.filter((receipt) => receipt.target === target).length,
      ).toBeLessThanOrEqual(2);
      expect(
        new Set(
          acceptedReceipts
            .filter((receipt) => receipt.target === target)
            .map((receipt) => receipt.id),
        ).size,
      ).toBe(1);
    }
    for (const [entity, expected] of [
      ["recording", fixture.selections[0].recording.id],
      ["release", fixture.selections[0].recording.release!.id],
      ["playlist", fixture.selections[0].detail.id],
    ]) {
      const requests = fixture.requests.filter(
        (request) =>
          request.path === "/fixture/v1/artwork" &&
          (request.body as { entity: string }).entity === entity,
      );
      expect(requests).toHaveLength(entity === "playlist" ? 2 : 1);
      expect(
        requests.every(
          (request) =>
            JSON.stringify(request.body) ===
            JSON.stringify({ entity, provider_id: expected }),
        ),
      ).toBe(true);
    }
    expect(await saved()).toEqual(before);
    const peer = await privateAccountDenials(
      browser,
      page,
      baseURL,
      evidence,
      [paths.recording, paths.release, ...paths.copies],
      ready,
    );
    evidence.phase("revocation");
    const revoked = await revokedPublication(
      page,
      fixture,
      installation,
      evidence,
    );
    const retainedCalls = fixture.requests.filter((request) =>
      request.path.endsWith("/artwork"),
    ).length;
    const enqueuedBeforeRetry = evidence.enqueues.length;
    const playlistRetry = await retainedRetry(
      page,
      `/playlists/${copyIds[0]}`,
      paths.copies[0],
      fixture.covers.playlist,
    );
    const rendererRecovery =
      browser.browserType().name() === "webkit"
        ? await coalescedRendererRetry(
            page,
            `/albums/${release}`,
            paths.release,
            paths.recording,
            fixture.covers.release,
          )
        : await lateRendererRetry(
            page,
            `/albums/${release}`,
            paths.release,
            fixture.covers.release,
          );
    expect(
      fixture.requests.filter((request) => request.path.endsWith("/artwork")),
    ).toHaveLength(retainedCalls);
    await evidence.settle();
    expect(evidence.enqueues).toHaveLength(enqueuedBeforeRetry);
    expect(await json(page, `${paths.release}/availability`)).toEqual({
      can_fetch: false,
    });
    expect(await json(page, `${paths.copies[0]}/availability`)).toEqual({
      can_fetch: false,
    });
    evidence.phase("logout");
    const loggedOut = await loggedOutPublication(
      browser,
      page,
      baseURL,
      fixture,
      connected.service,
      evidence,
      peer,
      [
        { path: paths.recording, bytes: fixture.covers.recording },
        { path: paths.release, bytes: fixture.covers.release },
        ...paths.copies.map((path) => ({
          path,
          bytes: fixture.covers.playlist,
        })),
      ],
    );
    const guardCopies = [revoked.copy, loggedOut.copy];
    const guardRecordingIds = guardCopies.map((copy) => {
      const id = copy.entries[0].recording_id;
      if (!id)
        throw new Error(
          "A guard import must retain its exact recording identity",
        );
      return id;
    });
    const downloadsAfter = await downloadAbsence(
      page,
      owner.id,
      [recording, ...guardRecordingIds],
      [...copyIds, ...guardCopies.map((copy) => copy.playlist.id)],
    );
    expect(downloadsAfter).toEqual({
      recordings: 3,
      playlists: 4,
      reads: 7,
      status: 404,
    });
    const after = await saved();
    expect(after.playlists.map((playlist) => playlist.id).sort()).toEqual(
      [...before.playlists, ...guardCopies.map((copy) => copy.playlist)]
        .map((playlist) => playlist.id)
        .sort(),
    );
    for (const copy of guardCopies) {
      expect(
        after.playlists.find((playlist) => playlist.id === copy.playlist.id),
      ).toEqual(copy.playlist);
    }
    const additions = after.feed.operations.filter(
      (event) => event.sequence > before.feed.cursor,
    );
    expect(additions).toHaveLength(2);
    const guardRecordings: string[] = [];
    const guardReleases: string[] = [];
    for (const [index, event] of additions.entries()) {
      if (
        !("publication" in event) ||
        event.publication.kind !== "playlist_imported"
      )
        throw new Error(
          "Only the two deliberate guard imports may publish collection events",
        );
      const publication = event.publication.snapshot;
      const copy = guardCopies[index];
      expect(publication.playlist_id).toBe(copy.playlist.id);
      expect(publication.name).toBe(copy.playlist.name);
      expect(publication.origin).toEqual(copy.origin);
      expect(
        publication.entries.map((entry) => [
          entry.entry_id,
          entry.recording_id,
          entry.position,
          entry.origin,
        ]),
      ).toEqual(
        copy.entries.map((entry) => [
          entry.entry_id,
          entry.recording_id,
          entry.position,
          entry.origin,
        ]),
      );
      expect(publication.recordings).toHaveLength(1);
      const imported = publication.recordings[0];
      const metadata = fixture.selections[index + 1].recording;
      expect(imported.metadata).toEqual({
        ...metadata,
        disc_number: null,
        track_number: null,
      });
      expect(imported.recording_id).toBe(copy.entries[0].recording_id);
      if (!imported.release_id)
        throw new Error(
          "The owned guard import requires its exact release identity",
        );
      guardRecordings.push(imported.recording_id);
      guardReleases.push(imported.release_id);
      expect(
        after.catalog.items.find((item) => item.id === imported.recording_id),
      ).toEqual({
        id: imported.recording_id,
        library_id: imported.library_id,
        title: metadata.title,
        artist: metadata.artist,
        inherited_title: metadata.title,
        inherited_artist: metadata.artist,
        duration_ms: metadata.duration_ms,
        available: false,
        liked: false,
        release_id: imported.release_id,
        album: metadata.release!.title,
      });
      expect(
        after.albums.items.find((item) => item.id === imported.release_id),
      ).toEqual({
        id: imported.release_id,
        library_id: imported.library_id,
        title: metadata.release!.title,
        artist: null,
        artist_id: null,
        track_count: 1,
        duration_ms: metadata.duration_ms,
        artwork_recording_id: imported.recording_id,
      });
    }
    expect(after.catalog.total).toBe(before.catalog.total + 2);
    expect(after.catalog.items.map((item) => item.id).sort()).toEqual(
      [
        ...before.catalog.items.map((item) => item.id),
        ...guardRecordings,
      ].sort(),
    );
    expect(after.albums.total).toBe(before.albums.total + 2);
    expect(after.albums.items.map((item) => item.id).sort()).toEqual(
      [...before.albums.items.map((item) => item.id), ...guardReleases].sort(),
    );
    expect(after.copies).toEqual(before.copies);
    expect(after.metadata).toEqual(before.metadata);
    expect(after.albumMetadata).toEqual(before.albumMetadata);
    expect(after.catalog.items.find((item) => item.id === recording)).toEqual(
      before.catalog.items.find((item) => item.id === recording),
    );
    expect(after.albums.items.find((item) => item.id === release)).toEqual(
      before.albums.items.find((item) => item.id === release),
    );
    expect(after.activity).toEqual(before.activity);
    expect(after.sources).toEqual(before.sources);
    expect(after.feed.has_more).toBe(false);
    expect(
      after.feed.operations.filter(
        (event) => event.sequence <= before.feed.cursor,
      ),
    ).toEqual(before.feed.operations);
    expect(await ownedOriginals()).toEqual(original);
    await writeFile(
      "/evidence/artwork-downloads.json",
      JSON.stringify({ before: downloadsBefore, after: downloadsAfter }),
      { mode: 0o600 },
    );
    expect(fixture.violations).toBe(0);
    for (const request of fixture.requests) {
      expect(request.cookie).toBeUndefined();
      expect(request.protocol).toBe("1");
      expect(
        request.authorization === undefined
          ? request.path === "/fixture/v1/manifest"
          : request.authorization === `Bearer ${fixture.token}`,
      ).toBe(true);
    }
    const privateValues = [
      ...fixture.privateValues,
      ...Object.values(titles).flat(),
      peer.password,
      "docker browser test password",
      "/music/owned-original.wav",
      "/owned-music/owned-original.wav",
    ];
    const diagnostics = await evidence.verify(privateValues, ready);
    await info.attach("collection-artwork-evidence", {
      contentType: "application/json",
      body: JSON.stringify(
        {
          imageHashes: hashes,
          originalHash: hash(original[0].bytes),
          originalBytes: original[0].size,
          copies: copyIds.length,
          duplicateEntries: 4,
          readyJobs: ready.map(({ id }) => id),
          revokedJob: revoked.receipt.id,
          loggedOutJob: loggedOut.receipt.id,
          rendererRecovery,
          playlistRetry,
          downloadStatusReads: downloadsBefore.reads + downloadsAfter.reads,
          ...diagnostics,
        },
        null,
        2,
      ),
    });
  } finally {
    await fixture.close();
  }
});
