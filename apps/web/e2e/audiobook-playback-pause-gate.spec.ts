import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { audiobookPlaybackEvidence } from "./fixtures/audiobook-playback-evidence";
import { audiobookPlaybackGateFixture } from "./fixtures/audiobook-playback-gates";
import { audiobookMediaState } from "./fixtures/audiobook-playback-state";

type Schema = components["schemas"];
type PauseControl = {
  native: number;
  exact: number;
  status: number;
  release(): void;
};

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_PLAYBACK_GATES_E2E !== "1",
  "Requires explicit owned audiobook playback gate opt-in",
);

test("pause checkpoint waits for native progress submission and its exact held acknowledgement", async ({
  context,
  page,
  browserName,
}) => {
  test.setTimeout(25_000);
  page.setDefaultTimeout(5000);
  let releaseReceipt!: () => void;
  let receivedReceipt!: () => void;
  const receiptHeld = new Promise<void>((resolve) => {
    receivedReceipt = resolve;
  });
  const receiptReleased = new Promise<void>((resolve) => {
    releaseReceipt = resolve;
  });
  const operation = randomUUID();
  const diagnostics = {
    failed: 0,
    native_pause: 0,
    exact_media: 0,
    submission_barrier: 0,
    receipt_barrier: 0,
    returned_before_submission: 0,
    returned_while_receipt_held: 0,
    acknowledged_when_returned: 0,
    wait_rejected: 0,
    receipt_operation: 0,
    delivered_status: 0,
  };
  const fixture = await audiobookPlaybackGateFixture(
    undefined,
    async (batch) => {
      diagnostics.receipt_operation = Number(
        batch.operations[0]?.id === operation,
      );
      receivedReceipt();
      await receiptReleased;
    },
  );
  const evidence = await audiobookPlaybackEvidence(
    context,
    fixture.origin,
    browserName,
    [],
  );
  let completion: Promise<unknown> | undefined;
  let settled = 0;
  try {
    await page.goto(fixture.origin);
    await expect.poll(() => evidence.pending()).toBe(0);
    await evidence.arm(fixture.target);
    const path = fixture.target.audio[0].path;
    await page
      .getByRole("button", { name: "Play owned audio", exact: true })
      .click();
    await expect
      .poll(async () => {
        const state = await audiobookMediaState(page, path);
        return (
          state.exact &&
          state.playing &&
          state.position > 0.2 &&
          state.position < 3
        );
      })
      .toBe(true);
    await expect.poll(() => evidence.pending()).toBe(0);
    const batch: Schema["SyncBatch"] = {
      device_id: randomUUID(),
      operations: [
        {
          id: operation,
          action: {
            kind: "progress_set",
            recording_id: fixture.target.recordings[0],
            position_ms: 0,
            speed: 1,
          },
        },
      ],
    };
    await page.evaluate(
      ({ path, account, batch }) => {
        const audio = document.querySelector<HTMLAudioElement>(
          "audio[data-hito-player]",
        )!;
        let release!: () => void;
        const released = new Promise<void>((resolve) => {
          release = resolve;
        });
        const control: PauseControl = {
          native: 0,
          exact: 0,
          status: 0,
          release,
        };
        (globalThis as unknown as Record<string, unknown>).__hitoPauseGate =
          control;
        audio.addEventListener(
          "pause",
          async (event) => {
            control.native = Number(event.isTrusted);
            control.exact = Number(
              event.target === audio &&
                audio.isConnected &&
                audio.paused &&
                !audio.ended &&
                audio.error === null &&
                document.querySelectorAll("audio[data-hito-player]").length ===
                  1 &&
                audio.getAttribute("src") === path &&
                audio.currentSrc === new URL(path, location.href).href,
            );
            const action = batch.operations[0].action;
            if (action.kind !== "progress_set") return;
            action.position_ms = Math.floor(audio.currentTime * 1000);
            action.speed = audio.playbackRate;
            await released;
            try {
              const response = await fetch("/api/v1/sync", {
                method: "POST",
                headers: {
                  "content-type": "application/json",
                  "x-hito-request": "1",
                  "x-hito-account": account,
                },
                body: JSON.stringify(batch),
              });
              const bytes = await response.arrayBuffer();
              control.status = bytes.byteLength <= 4096 ? response.status : 0;
            } catch {
              control.status = -1;
            }
          },
          { once: true },
        );
        const button = document.createElement("button");
        button.textContent = "Pause owned audio";
        button.onclick = () => audio.pause();
        document.body.append(button);
      },
      { path, account: fixture.target.account, batch },
    );
    const checkpoint = await evidence.pauseCheckpoint(page, path);
    await page
      .getByRole("button", { name: "Pause owned audio", exact: true })
      .click();
    completion = checkpoint()
      .then(() => {
        settled = 1;
        diagnostics.acknowledged_when_returned = Number(
          evidence
            .accepted()
            .some((batch) => batch.operations[0].id === operation),
        );
      })
      .catch(() => {
        diagnostics.wait_rejected = 1;
      });
    await page.waitForFunction(() => {
      const control = (globalThis as unknown as Record<string, unknown>)
        .__hitoPauseGate as PauseControl;
      return control.native === 1 && control.exact === 1;
    });
    diagnostics.submission_barrier = await page.evaluate(() => {
      const control = (globalThis as unknown as Record<string, unknown>)
        .__hitoPauseGate as PauseControl;
      const audio = document.querySelector<HTMLAudioElement>(
        "audio[data-hito-player]",
      );
      return Number(
        control.native === 1 &&
          control.exact === 1 &&
          control.status === 0 &&
          audio?.paused,
      );
    });
    diagnostics.returned_before_submission = settled;
    await page.evaluate(() => {
      (
        (globalThis as unknown as Record<string, unknown>)
          .__hitoPauseGate as PauseControl
      ).release();
    });
    await bounded(receiptHeld);
    diagnostics.receipt_barrier = await page.evaluate(() => {
      const control = (globalThis as unknown as Record<string, unknown>)
        .__hitoPauseGate as PauseControl;
      return Number(
        control.native === 1 && control.exact === 1 && control.status === 0,
      );
    });
    diagnostics.returned_while_receipt_held = settled;
    releaseReceipt();
    await bounded(completion);
    await expect.poll(() => evidence.pending()).toBe(0);
    const observed = await page.evaluate(() => {
      const control = (globalThis as unknown as Record<string, unknown>)
        .__hitoPauseGate as PauseControl;
      return {
        native: control.native,
        exact: control.exact,
        status: control.status,
      };
    });
    diagnostics.native_pause = observed.native;
    diagnostics.exact_media = observed.exact;
    diagnostics.delivered_status = observed.status;
  } catch {
    diagnostics.failed++;
  } finally {
    releaseReceipt();
    try {
      await context.close();
    } catch {
      diagnostics.failed++;
    }
    try {
      await evidence.finish();
    } catch {
      diagnostics.failed++;
    }
    try {
      await fixture.close();
    } catch {
      diagnostics.failed++;
    }
    if (completion)
      await bounded(completion).catch(() => {
        diagnostics.failed++;
      });
  }
  const passed =
    diagnostics.failed +
      diagnostics.wait_rejected +
      diagnostics.returned_before_submission +
      diagnostics.returned_while_receipt_held ===
      0 &&
    diagnostics.native_pause === 1 &&
    diagnostics.exact_media === 1 &&
    diagnostics.submission_barrier === 1 &&
    diagnostics.receipt_barrier === 1 &&
    diagnostics.receipt_operation === 1 &&
    diagnostics.acknowledged_when_returned === 1 &&
    diagnostics.delivered_status === 200 &&
    fixture.counts.sync === 1 &&
    fixture.counts.errors === 0 &&
    evidence.counts.invalid +
      evidence.counts.forbidden +
      evidence.counts.workflow_failures ===
      0 &&
    evidence.counts.progress_submitted === 1 &&
    evidence.counts.progress_accepted === 1;
  const report = JSON.stringify({
    passed,
    ...diagnostics,
    server: fixture.counts,
    evidence: evidence.counts,
  });
  if (Buffer.byteLength(report) > 4096)
    throw new Error("Owned pause gate diagnostic bound");
  console.log(`audiobook_playback_pause_gate ${report}`);
  if (!passed)
    throw new Error("The owned pause progress checkpoint regression failed");
});

async function bounded<T>(work: Promise<T>) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned pause gate deadline")),
          5000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
