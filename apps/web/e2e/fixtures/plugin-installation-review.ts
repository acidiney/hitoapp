import { isDeepStrictEqual } from "node:util";
import { expect, type Page, type Request } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

export async function reviewPluginInstallation(
  page: Page,
  service: Schema<"PluginService">,
  expectedName = service.name,
  activate = () =>
    page
      .getByRole("button", { name: "Review connection", exact: true })
      .click(),
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
    const heading = page.getByRole("heading", {
      name: "Review connection",
      exact: true,
    });
    await expect(heading).toBeVisible();
    const review = heading.locator("..");
    const values = review.getByRole("definition");
    await expect(values).toHaveCount(3);
    expect((await values.nth(0).innerText()) === body.manifest.name).toBe(true);
    expect(
      (await values.nth(1).innerText()) ===
        body.manifest.capabilities
          .map((value) => value.replaceAll("_", " "))
          .join(", "),
    ).toBe(true);
    const persistence = body.manifest.content_policy?.source_persistence;
    const disclosure =
      persistence === "playback_only"
        ? "Playback only. No new offline downloads. Completed copies are retained."
        : persistence === "explicit_download"
          ? "Explicit downloads declared, subject to Hito's checks. This does not grant a licence."
          : "No persistence declaration. Download permission is unknown; Hito's checks still apply.";
    expect((await values.nth(2).innerText()) === disclosure).toBe(true);
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
