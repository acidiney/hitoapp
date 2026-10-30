import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  observePluginInstallations,
  reviewPluginInstallation,
} from "./fixtures/plugin-installation-review";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

async function observeOutcome(result: Promise<string>, timeoutMs: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      result,
      new Promise<string>((resolve) => {
        timer = setTimeout(() => resolve("pending"), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function browserControls() {
  const token = "private-owned-review-gate-token";
  const manifest = {
    id: "org.hito.owned-review-gate",
    name: "Owned review gate",
    protocol_version: 1,
    capabilities: ["search", "sources"],
    content_policy: { source_persistence: "playback_only" },
  } satisfies Schema<"Manifest">;
  const preview = {
    manifest,
    digest: "private-owned-review-gate-digest",
  } satisfies Schema<"PluginInstallationPreview">;
  let release!: () => void;
  const heldBody = new Promise<void>((resolve) => {
    release = resolve;
  });
  const counters = { previews: 0, released: 0, installations: 0 };
  const server = createServer(async (request, response) => {
    request.resume();
    if (request.method === "GET" && request.url === "/") {
      response.writeHead(200, { "content-type": "text/html" })
        .end(`<!doctype html>
<label>Bearer token (optional)<input type="password" value="${token}"></label>
<button id="review">Review connection</button>
<button id="release">Release declaration body</button>
<button id="burst">Send installation burst</button>
<p role="status" id="status">Ready</p><div id="declaration"></div>
<script>
const draft = { service_id: "owned-review-service", service_revision: 1, bearer_token: ${JSON.stringify(token)} };
document.querySelector("#review").onclick = async () => {
  const response = await fetch("/api/v1/plugins/preview", { method: "POST", body: JSON.stringify(draft) });
  const preview = await response.json();
  const review = document.querySelector("#declaration");
  const heading = document.createElement("h3");
  heading.textContent = "Review connection";
  review.append(heading);
  const values = document.createElement("dl");
  for (const [term, value] of [["Name", preview.manifest.name], ["Declared capabilities", preview.manifest.capabilities.join(", ")], ["Source persistence", "Playback only. No new offline downloads. Completed copies are retained."]]) {
    const title = document.createElement("dt"); title.textContent = term;
    const detail = document.createElement("dd"); detail.textContent = value;
    values.append(title, detail);
  }
  review.append(values);
  const accept = document.createElement("button"); accept.textContent = "Accept and connect"; review.append(accept);
};
document.querySelector("#release").onclick = async () => {
  await fetch("/release-preview", { method: "POST" });
  document.querySelector("#status").textContent = "Declaration body released";
};
document.querySelector("#burst").onclick = async () => {
  for (const bearer_token of [${JSON.stringify(token)}.repeat(64), draft.bearer_token, draft.bearer_token, draft.bearer_token])
    await fetch("/api/v1/plugins", { method: "POST", body: JSON.stringify({ ...draft, bearer_token }) });
  document.querySelector("#status").textContent = "Installation burst sent";
};
</script>`);
      return;
    }
    if (
      request.method === "POST" &&
      request.url === "/api/v1/plugins/preview"
    ) {
      counters.previews++;
      response.writeHead(200, { "content-type": "application/json" });
      response.flushHeaders();
      await heldBody;
      if (!response.destroyed) response.end(JSON.stringify(preview));
      return;
    }
    if (request.method === "POST" && request.url === "/release-preview") {
      counters.released++;
      release();
      response.writeHead(204).end();
      return;
    }
    if (request.method === "POST" && request.url === "/api/v1/plugins") {
      counters.installations++;
      response.writeHead(201, { "content-type": "application/json" }).end("{}");
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  const service = {
    id: "owned-review-service",
    name: manifest.name,
    base_url: address,
    revision: 1,
  } satisfies Schema<"PluginService">;
  return {
    address,
    service,
    token,
    counters,
    close: async () => {
      release();
      server.closeAllConnections();
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          new Promise<void>((resolve, reject) =>
            server.close((error) =>
              error
                ? reject(new Error("Owned browser controls did not close"))
                : resolve(),
            ),
          ),
          new Promise<never>((_, reject) => {
            timer = setTimeout(
              () =>
                reject(
                  new Error("Owned browser cleanup exceeded its deadline"),
                ),
              3_000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

test("the preview deadline also bounds a held HTTP response body", async ({
  page,
}) => {
  test.setTimeout(10_000);
  const fixture = await browserControls();
  let errors = 0;
  let result: Promise<string> | undefined;
  page.on("pageerror", () => errors++);
  try {
    await page.goto(fixture.address);
    result = reviewPluginInstallation(
      page,
      fixture.service,
      fixture.service.name,
      undefined,
      200,
    ).then(
      () => "accepted",
      (error: unknown) =>
        error instanceof Error &&
        error.message === "The service preview exceeded its deadline"
          ? "deadline"
          : "unexpected_error",
    );
    const outcome = await observeOutcome(result, 600);
    await page
      .getByRole("button", { name: "Release declaration body", exact: true })
      .click();
    await expect(page.getByRole("status")).toHaveText(
      "Declaration body released",
    );
    expect((await observeOutcome(result, 1_000)) !== "pending").toBe(true);
    expect(outcome === "deadline").toBe(true);
    expect(
      fixture.counters.previews === 1 &&
        fixture.counters.released === 1 &&
        fixture.counters.installations === 0 &&
        errors === 0,
    ).toBe(true);
  } finally {
    await fixture.close();
    if (result) await observeOutcome(result, 1_000);
  }
});

test("installation evidence keeps total counts while bounding credential samples and raw request bodies", async ({
  page,
}) => {
  test.setTimeout(10_000);
  const fixture = await browserControls();
  const { evidence, stop } = observePluginInstallations(page, {
    maxSamples: 2,
    maxBodyBytes: 512,
  });
  let errors = 0;
  page.on("pageerror", () => errors++);
  try {
    await page.goto(fixture.address);
    await page
      .getByRole("button", { name: "Send installation burst", exact: true })
      .click();
    await expect(page.getByRole("status")).toHaveText(
      "Installation burst sent",
    );
    expect(evidence.total === 4 && fixture.counters.installations === 4).toBe(
      true,
    );
    expect(evidence.samples.length === 2 && evidence.overflow).toBe(true);
    expect(
      evidence.samples.every((sample) => sample.bearer_token === fixture.token),
    ).toBe(true);
    expect(errors === 0).toBe(true);
  } finally {
    stop();
    await fixture.close();
  }
});
