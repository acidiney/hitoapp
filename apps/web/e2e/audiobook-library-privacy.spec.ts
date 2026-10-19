import { writeFile } from "node:fs/promises";
import { test } from "@playwright/test";
import {
  prepareLibrary,
  type LibraryFixture,
} from "./fixtures/audiobook-library-state";
import { libraryEvidence } from "./fixtures/audiobook-library-evidence";
import { libraryDelivery } from "./fixtures/audiobook-library-delivery";
import { libraryPrivacyControls } from "./fixtures/audiobook-library-privacy-controls";

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
test("packaged audiobook library privacy controls", async ({
  browser,
  page,
  context,
  baseURL,
  browserName,
}) => {
  if (baseURL !== "http://localhost:8080")
    throw new Error("The owned library control origin is invalid");
  const budget = { requests: 0, writes: 0 };
  const evidence = await libraryEvidence(context, baseURL);
  const delivery = await libraryDelivery(context, baseURL, evidence.remember);
  let fixture: LibraryFixture | null = null;
  let controls: Awaited<ReturnType<typeof libraryPrivacyControls>> = [];
  let outcome = "failed";
  const checks = { controls: false, privacy: false, originals: false };
  try {
    fixture = await prepareLibrary(page, baseURL, evidence.remember, budget);
    controls = await libraryPrivacyControls(browser, context, fixture, baseURL);
    checks.controls =
      controls.length === 3 &&
      controls.reduce(
        (total, value) => total + value.requests,
        delivery.counts.requests,
      ) <= 250;
    checks.originals = await fixture.originalsUnchanged();
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
    if (!Object.values(checks).every(Boolean)) outcome = "failed";
    await writeFile(
      "/evidence/summary.json",
      JSON.stringify({
        workflow: "privacy",
        browser: browserName,
        outcome,
        stage: "selftests",
        checks,
        public: budget,
        transport: delivery.counts,
        logs: evidence.counts,
        controls,
      }),
      { mode: 0o600 },
    );
  }
  if (outcome !== "passed")
    throw new Error(
      "The packaged native console boundary controls failed; see the private numeric summary",
    );
});
