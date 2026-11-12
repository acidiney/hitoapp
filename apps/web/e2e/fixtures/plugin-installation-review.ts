import { isDeepStrictEqual } from "node:util";
import { expect, type Page, type Request } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { declaredCapabilities } from "../../src/features/plugins/model/capabilityRows";

export async function reviewPluginInstallation(
  page: Page,
  service: Schema<"PluginService">,
  expectedName = service.name,
  activate = () =>
    page.getByRole("button", { name: "Continue", exact: true }).click(),
  deadlineMs = 15_000,
) {
  const readReview = async () => {
    const token = await page
      .getByLabel("Bearer token (optional)", { exact: true })
      .inputValue();
    const [response] = await Promise.all([
      page.waitForResponse(
        (candidate) =>
          new URL(candidate.url()).pathname === "/api/v1/plugins/preview" &&
          candidate.request().method() === "POST",
        { timeout: deadlineMs },
      ),
      activate(),
    ]);
    if (response.status() !== 200)
      throw new Error("The service preview did not return its declaration");
    const bytes = await response.body();
    if (bytes.length > 524_288)
      throw new Error("The service preview exceeded its response bound");
    const body: Schema<"PluginInstallationPreview"> = JSON.parse(
      bytes.toString(),
    );
    const sent: Schema<"PreviewPluginInstallation"> = response
      .request()
      .postDataJSON();
    expect(
      isDeepStrictEqual(sent, {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: token || null,
      }) &&
        Object.keys(body).sort().join(",") === "digest,manifest" &&
        body.manifest.name === expectedName &&
        body.digest.length > 0,
    ).toBe(true);
    const review = page.getByRole("dialog", {
      name: "Before you connect",
      exact: true,
    });
    await expect(review).toBeVisible();
    const rows = declaredCapabilities(body.manifest.capabilities);
    const items = review
      .getByRole("list", { name: "What it will do" })
      .getByRole("listitem");
    await expect(items).toHaveCount(rows.length);
    for (const [index, row] of rows.entries()) {
      expect(await items.nth(index).locator("strong").innerText()).toBe(
        row.name,
      );
      expect(await items.nth(index).locator("span").last().innerText()).toBe(
        row.what,
      );
    }
    const persistence = body.manifest.content_policy?.source_persistence;
    const disclosure =
      persistence === "playback_only"
        ? "It only plays audio. Hito won't save new copies, and copies you already have stay."
        : persistence === "explicit_download"
          ? "It allows saving audio when you ask. Hito still checks before saving anything, and this isn't a licence."
          : "It doesn't say whether its audio may be saved. Hito still asks before saving anything.";
    await expect(review.getByText(disclosure, { exact: true })).toBeVisible();
    expect(
      await review
        .getByRole("button")
        .evaluateAll((buttons) => buttons.map((button) => button.textContent)),
    ).toEqual(["Accept and connect", "Back"]);
    await expect(review.getByLabel("Bearer token (optional)")).toHaveCount(0);
    await expect(review.locator("a,img,iframe,script,style,link")).toHaveCount(
      0,
    );
    await expect(
      review.getByRole("button", { name: "Accept and connect", exact: true }),
    ).toBeEnabled();
    return { body, sent };
  };
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      readReview(),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("The service preview exceeded its deadline")),
          deadlineMs,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export function observePluginInstallations(
  page: Page,
  limits: { maxSamples?: number; maxBodyBytes?: number } = {},
) {
  const maxSamples = limits.maxSamples ?? 8;
  const maxBodyBytes = limits.maxBodyBytes ?? 32_768;
  const samples: Schema<"InstallPlugin">[] = [];
  const evidence = {
    total: 0,
    samples,
    overflow: false,
  };
  const observe = (request: Request) => {
    if (
      request.method() !== "POST" ||
      new URL(request.url()).pathname !== "/api/v1/plugins"
    )
      return;
    evidence.total++;
    if (evidence.samples.length >= maxSamples) {
      evidence.overflow = true;
      return;
    }
    const raw = request.postData();
    if (raw === null || Buffer.byteLength(raw) > maxBodyBytes) {
      evidence.overflow = true;
      return;
    }
    try {
      const sample: Schema<"InstallPlugin"> = JSON.parse(raw);
      evidence.samples.push(sample);
    } catch {
      evidence.overflow = true;
    }
  };
  page.on("request", observe);
  return {
    evidence,
    stop: () => page.off("request", observe),
  };
}
