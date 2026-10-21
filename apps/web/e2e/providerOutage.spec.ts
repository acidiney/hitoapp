import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import {
  providerOutageProvider,
  type OutageProvider,
} from "./fixtures/providerOutageProvider";
import {
  assertOutagePreserved,
  outageLibraryState,
  outageReader,
  outageWrite,
  prepareOutageLibrary,
  type OutageSetupPhase,
  type OutageResolutionDiagnostic,
} from "./fixtures/providerOutageLibrary";
import {
  hash,
  outageControl,
  providerOutageEvidence,
  waitOutageControl,
} from "./fixtures/providerOutageEvidence";
import {
  playThroughOutage,
  outageMediaFacts,
  type OutageMediaFacts,
  type OutagePlaybackPhase,
} from "./fixtures/providerOutagePlayback";

type Schema = components["schemas"];
type OutagePhase =
  | OutageSetupPhase
  | OutagePlaybackPhase
  | "configuration"
  | "migration"
  | "playback"
  | "preservation"
  | "runtime_logs"
  | "evidence"
  | "browser_cleanup"
  | "provider_cleanup";
test.use({ trace: "off", screenshot: "off", video: "off" });
test.skip(
  process.env.HITO_PROVIDER_OUTAGE_E2E !== "1",
  "Requires an owned disposable upgrade server",
);

test("a populated version54 library upgrades and plays through truthful provider outages without losing private state", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(180_000);
  let phase: OutagePhase = "configuration";
  let failure: OutagePhase | null = null;
  let provider: OutageProvider | null = null;
  let media: OutageMediaFacts | null = null;
  const resolutions: Partial<
    Record<OutageResolutionDiagnostic["target"], OutageResolutionDiagnostic>
  > = {};
  const started = performance.now();
  try {
    if (
      baseURL !== "http://localhost:8080" ||
      process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1" ||
      !/^[0-9a-f]{32}$/.test(process.env.HITO_OUTAGE_RUN_ID ?? "")
    )
      throw new Error("Owned browser configuration rejected");
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(10_000);
    const password = `owned-upgrade-${randomUUID()}`;
    const evidence = await providerOutageEvidence(context, page, baseURL);
    evidence.register([password]);
    provider = await providerOutageProvider();
    evidence.register(provider.privateValues);
    const read = outageReader(page);
    const library = await prepareOutageLibrary(
      page,
      read,
      provider,
      password,
      (setupPhase) => {
        phase = setupPhase;
      },
      (diagnostic) => {
        resolutions[diagnostic.target] = {
          ...resolutions[diagnostic.target],
          ...diagnostic,
        };
      },
    );
    evidence.register(library.privateValues);
    evidence.register((await context.cookies()).map((cookie) => cookie.value));
    phase = "snapshot";
    const before = await outageLibraryState(page, read, library);
    phase = "migration";
    await page.goto("about:blank");
    await outageControl("legacy-ready", "legacy_ready", {
      state_sha256: hash(before),
      assets: evidence.assets,
    });
    const migrated = await waitOutageControl("migrated", "migrated", 90_000);
    expect(
      migrated.previous_version === 54 &&
        migrated.current_version === 55 &&
        migrated.applied_count === 1,
    ).toBe(true);
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const afterMigration = await outageLibraryState(page, read, library);
    expect(isDeepStrictEqual(before, afterMigration)).toBe(true);
    const callsBeforeReplay = { ...provider.counts };
    expect(
      library.legacyResolutions.length === 3 &&
        new Set(
          library.legacyResolutions.map((item) => item.request.request_id),
        ).size === 3 &&
        library.legacyResolutions.every(
          (item) =>
            item.receipt.request_id === item.request.request_id &&
            item.receipt.recording_id === item.recording,
        ),
    ).toBe(true);
    for (const resolution of library.legacyResolutions) {
      const replay = await outageWrite<Schema["SourceResolution"]>(
        page,
        library.user.id,
        "post",
        `/api/v1/recordings/${resolution.recording}/sources/resolve`,
        resolution.request,
      );
      expect(isDeepStrictEqual(replay, resolution.receipt)).toBe(true);
      expect(isDeepStrictEqual(callsBeforeReplay, provider.counts)).toBe(true);
    }
    phase = "playback";
    evidence.arm(library.user.id, [library.ids.retry, library.ids.healthy]);
    const playback = await playThroughOutage(
      page,
      library,
      provider,
      evidence,
      (playbackPhase) => {
        phase = playbackPhase;
      },
    );
    phase = "preservation";
    const after = await outageLibraryState(page, read, library);
    assertOutagePreserved(before, after, library);
    for (const copy of Object.values(library.copies)) {
      const response = await page.request.get(
        `/api/v1/playlists/${copy.playlist.id}/download`,
        { timeout: 10_000 },
      );
      try {
        const bytes = await response.body();
        expect(
          response.status() === 404 &&
            bytes.length === 30 &&
            bytes.toString() ===
              JSON.stringify({ error: "Resource not found" }),
        ).toBe(true);
      } finally {
        await response.dispose();
      }
    }
    await evidence.drain();
    expect(
      evidence.requests.length === 4 &&
        evidence.receipts.length === 3 &&
        evidence.receipts.filter(
          (receipt) => receipt.outcome === "provider_unavailable",
        ).length === 2 &&
        evidence.receipts.filter((receipt) => receipt.outcome === "ready")
          .length === 1 &&
        evidence.completions.length === 3,
    ).toBe(true);
    expect(
      provider.counts.unavailable > 0 &&
        provider.counts.audioGet > 0 &&
        provider.counts.rejected === 0,
    ).toBe(true);
    phase = "runtime_logs";
    const logs = await evidence.serverLogCheck();
    phase = "evidence";
    await writeFile(
      "/evidence/provider-outage.json",
      JSON.stringify({
        event: "provider_outage_packaged_verified",
        migration: migrated,
        playback,
        preserved_migration_state: hash(before) === hash(afterMigration),
        original_hashes: after.originals,
        legacy_receipts_replayed: library.legacyResolutions.length,
        provider: provider.counts,
        logs,
        ...evidence.summary(),
        elapsed_ms: Math.round(performance.now() - started),
      }),
    );
  } catch {
    failure = phase;
    try {
      media = await outageMediaFacts(page);
    } catch {
      media = null;
    }
  } finally {
    try {
      await context.close();
    } catch {
      failure ??= "browser_cleanup";
    }
    try {
      await provider?.close();
    } catch {
      failure ??= "provider_cleanup";
    }
  }
  if (failure) {
    await writeFile(
      "/evidence/setup-failure.json",
      JSON.stringify({
        event: "provider_outage_failed",
        phase: failure,
        media,
        resolutions,
        provider: provider?.counts ?? null,
        elapsed_ms: Math.round(performance.now() - started),
      }),
    );
    const error = new Error(`Owned provider outage gate failed at ${failure}`);
    error.stack = error.message;
    throw error;
  }
});
