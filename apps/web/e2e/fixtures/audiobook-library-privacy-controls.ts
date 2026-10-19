import { expect, type Browser, type BrowserContext } from "@playwright/test";
import type { components } from "../../src/generated";
import { captureConsolePrivacy } from "./console-privacy";
import { libraryConsoleBoundary } from "./audiobook-library-console";
import type { LibraryFixture } from "./audiobook-library-state";
import {
  libraryDelivery,
  type LibraryNativeWitness,
} from "./audiobook-library-delivery";

export async function libraryPrivacyControls(
  browser: Browser,
  owner: BrowserContext,
  fixture: LibraryFixture,
  origin: string,
) {
  const page = await fixture.owner.read<
    components["schemas"]["AudiobookLibraryPage"]
  >("/api/v1/audiobooks/page?limit=60");
  const cursor = page.pagination.next_cursor;
  if (!cursor) throw new Error("The owned privacy control cursor is missing");
  fixture.remember(cursor);
  const url = `${origin}/audiobooks?cursor=${encodeURIComponent(cursor)}`;
  const storage = await owner.storageState();
  const results = [];
  for (const kind of [
    "javascript",
    "unowned_native",
    "owned_native",
  ] as const) {
    const context = await browser.newContext({
      baseURL: origin,
      storageState: storage,
    });
    const pending = new Set<Promise<void>>();
    const boundary = libraryConsoleBoundary(context, origin);
    const delivery =
      kind === "owned_native"
        ? await libraryDelivery(context, origin, () => {})
        : null;
    const privateValues = new Set([cursor]);
    const witness: LibraryNativeWitness = {
      phase: "lost",
      observed: false,
      acceptedStatus: null,
      failed: false,
    };
    let failures = 0;
    let requests = 0;
    context.on("request", () => {
      requests++;
    });
    context.on("requestfailed", (request) => {
      if (request.url() === url && request.method() === "GET")
        witness.failed = true;
    });
    if (delivery) boundary.networkProof(delivery.nativeWitness);
    await captureConsolePrivacy(
      context,
      (text, capture) => {
        if (capture === "snapshot") boundary.snapshot(text);
      },
      () => {
        failures++;
      },
      pending,
    );
    try {
      const control = await context.newPage();
      await control.goto("/audiobooks");
      await expect(
        control.getByRole("heading", {
          name: "Audiobooks",
          exact: true,
          level: 1,
        }),
      ).toBeVisible();
      if (kind === "javascript") {
        await control.evaluate(
          (address) =>
            console.error(`Web Inspector blocked ${address} from loading`),
          url,
        );
        await expect
          .poll(() =>
            boundary
              .inspect(privateValues, true)
              .findings.some(
                (entry) => entry.source === "application_snapshot",
              ),
          )
          .toBe(true);
      } else if (delivery) {
        delivery.arm(fixture.owner.user.id);
        delivery.disrupt("lost", new URL(url).pathname + new URL(url).search);
        await control.evaluate(
          async ({ address, account }) => {
            await fetch(address, {
              headers: {
                "x-inertia": "true",
                "x-hito-account": account,
                "x-hito-request": "1",
              },
            }).catch(() => {});
          },
          { address: url, account: fixture.owner.user.id },
        );
        await expect
          .poll(
            () =>
              boundary.inspect(privateValues, true).nativeNetworkDiagnostics,
          )
          .toBeGreaterThan(0);
        expect(
          delivery.counts.forwarded === 1 &&
            delivery.counts.lost === 1 &&
            delivery.counts.ownedFailures === 1 &&
            delivery.counts.invalid === 0,
        ).toBe(true);
      } else {
        await context.route(url, async (route) => {
          const request = route.request();
          const response = await route.fetch({
            timeout: 10_000,
            maxRedirects: 0,
            maxRetries: 0,
          });
          try {
            const bytes = await response.body();
            if (
              request.method() !== "GET" ||
              request.headers()["x-inertia"] !== "true" ||
              request.headers()["x-hito-account"] !== fixture.owner.user.id ||
              response.status() !== 200 ||
              response.headers()["x-inertia"] !== "true" ||
              response.headers()["cache-control"] !== "private, no-store" ||
              bytes.length > 524_288
            )
              throw new Error(
                "The owned native privacy control response was not accepted",
              );
            const accepted = JSON.parse(bytes.toString()) as {
              component?: unknown;
              props?: { user?: { id?: unknown }; books?: unknown[] };
            };
            if (
              accepted.component !== "AudiobookIndexPage" ||
              accepted.props?.user?.id !== fixture.owner.user.id ||
              accepted.props.books?.length !== 60
            )
              throw new Error(
                "The native privacy control response changed its scope",
              );
            witness.observed = true;
            witness.acceptedStatus = 200;
            await route.abort("failed");
          } catch {
            failures++;
            await route.abort("failed").catch(() => {});
          } finally {
            await response.dispose();
          }
        });
        await control.evaluate(
          async ({ address, account }) => {
            await fetch(address, {
              headers: {
                "x-inertia": "true",
                "x-hito-account": account,
                "x-hito-request": "1",
              },
            }).catch(() => {});
          },
          { address: url, account: fixture.owner.user.id },
        );
        await expect
          .poll(() => boundary.inspect(privateValues, true).privateConsole)
          .toBeGreaterThan(0);
        expect(
          witness.observed && witness.acceptedStatus === 200 && witness.failed,
        ).toBe(true);
      }
      while (pending.size) await Promise.allSettled([...pending]);
      const inspected = boundary.inspect(privateValues, true);
      expect(inspected.invalid + failures).toBe(0);
      expect(requests > 0 && requests <= 20).toBe(true);
      if (kind === "owned_native") {
        expect(inspected.privateConsole).toBe(0);
        expect(inspected.nativeNetworkDiagnostics).toBeGreaterThan(0);
      } else {
        expect(inspected.privateConsole).toBeGreaterThan(0);
        expect(inspected.nativeNetworkDiagnostics).toBe(0);
        expect(
          inspected.findings.some((entry) =>
            kind === "javascript"
              ? entry.source === "application_snapshot"
              : entry.args_count === 0 && entry.exact_owned_request === false,
          ),
        ).toBe(true);
      }
      let staleWitnessRejected = false;
      if (delivery) {
        await context.route(url, (route) => route.abort("failed"));
        await control.evaluate(
          async ({ address, account }) => {
            await fetch(address, {
              headers: {
                "x-inertia": "true",
                "x-hito-account": account,
                "x-hito-request": "1",
              },
            }).catch(() => {});
          },
          { address: url, account: fixture.owner.user.id },
        );
        await expect
          .poll(() => boundary.inspect(privateValues, true).privateConsole)
          .toBeGreaterThan(0);
        staleWitnessRejected = boundary
          .inspect(privateValues, true)
          .findings.some(
            (entry) =>
              entry.args_count === 0 &&
              entry.exact_owned_request === false &&
              entry.classified_native === false,
          );
        expect(staleWitnessRejected && delivery.counts.failures === 1).toBe(
          true,
        );
      }
      results.push({
        kind,
        requests,
        privateConsole: inspected.privateConsole,
        nativeNetworkDiagnostics: inspected.nativeNetworkDiagnostics,
        invalid: inspected.invalid,
        staleWitnessRejected,
      });
    } finally {
      await delivery?.close();
      await context.close();
    }
  }
  return results;
}
