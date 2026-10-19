import { writeFile } from "node:fs/promises";
import { test, expect } from "@playwright/test";
import {
  prepareLibrary,
  type LibraryFixture,
} from "./fixtures/audiobook-library-state";
import { libraryEvidence } from "./fixtures/audiobook-library-evidence";
import { libraryDelivery } from "./fixtures/audiobook-library-delivery";
import {
  traverseLibrary,
  controlNameCursors,
} from "./fixtures/audiobook-library-navigation";
import { recoverLibrary } from "./fixtures/audiobook-library-recovery";
import {
  explicitChanges,
  noListening,
  unchanged,
} from "./fixtures/audiobook-library-assertions";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_LIBRARY_E2E !== "1",
  "Requires the fresh owned packaged library server",
);

for (const workflow of ["traversal", "recovery"] as const) {
  test(`packaged audiobook library ${workflow}`, async ({
    page,
    context,
    baseURL,
    browserName,
  }) => {
    test.setTimeout(90_000);
    if (baseURL !== "http://localhost:8080")
      throw new Error("The owned library server origin is invalid");
    const budget = { requests: 0, writes: 0 };
    const evidence = await libraryEvidence(context, baseURL);
    const delivery = await libraryDelivery(context, baseURL, evidence.remember);
    evidence.networkProof(delivery.nativeWitness);
    let fixture: LibraryFixture | null = null;
    let stage = "prepare";
    let outcome = "failed";
    const checks = {
      state: false,
      originals: false,
      absence: false,
      navigation: false,
      privacy: false,
    };
    try {
      fixture = await prepareLibrary(page, baseURL, evidence.remember, budget);
      const before = await fixture.snapshot();
      const absence = await fixture.absence();
      delivery.arm(fixture.owner.user.id);
      stage = "navigation";
      if (workflow === "traversal") {
        await traverseLibrary(page, fixture, delivery);
        unchanged(before, await fixture.snapshot());
        stage = "control_cursor";
        const controlled = await controlNameCursors(fixture);
        explicitChanges(
          before,
          await fixture.snapshot(),
          controlled.operations,
          [...controlled.controls, ...before.owner.books],
          [...controlled.controls, ...before.owner.playlists],
        );
      } else {
        const recovered = await recoverLibrary(page, fixture, delivery);
        const playlists = before.owner.playlists
          .filter((value) => value.id !== recovered.deleted)
          .map((value) =>
            value.id === recovered.renamed.id ? recovered.renamed : value,
          )
          .sort((left, right) =>
            left.name < right.name
              ? -1
              : left.name > right.name
                ? 1
                : left.id < right.id
                  ? -1
                  : left.id > right.id
                    ? 1
                    : 0,
          );
        stage = "state";
        explicitChanges(
          before,
          await fixture.snapshot(),
          recovered.operations,
          recovered.books,
          playlists,
        );
      }
      checks.state = true;
      stage = "no_intent";
      unchanged(absence, await fixture.absence());
      checks.absence = true;
      expect(await fixture.originalsUnchanged()).toBe(true);
      checks.originals = true;
      await noListening(page, delivery, evidence, workflow === "recovery");
      checks.navigation = true;
      outcome = "passed";
    } catch {
      outcome = "failed";
    } finally {
      await delivery.close().catch(() => {
        outcome = "failed";
      });
      await fixture?.close().catch(() => {
        outcome = "failed";
      });
      await evidence.finish().catch(() => {
        outcome = "failed";
      });
      checks.privacy =
        evidence.counts.privateConsole === 0 &&
        evidence.counts.invalidLogs === 0 &&
        evidence.counts.pageErrors === 0;
      if (!checks.privacy) outcome = "failed";
      await writeFile(
        "/evidence/summary.json",
        JSON.stringify({
          workflow,
          browser: browserName,
          outcome,
          stage,
          checks,
          public: budget,
          transport: delivery.counts,
          logs: evidence.counts,
        }),
        { mode: 0o600 },
      );
    }
    if (outcome !== "passed")
      throw new Error(
        `Packaged audiobook library gate failed at ${stage}; see the bounded private numeric summary`,
      );
  });
}
