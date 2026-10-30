import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

export function artistReviewReader(page: Page) {
  let requests = 0;
  return async <T>(path: string): Promise<T> => {
    if (++requests > 100)
      throw new Error("Artist review public read budget exceeded");
    const response = await page.request.get(path, { timeout: 10_000 });
    try {
      expect(response.status()).toBe(200);
      const body = await response.body();
      if (body.length > 524_288)
        throw new Error("Artist review public read exceeded its bound");
      return JSON.parse(body.toString()) as T;
    } finally {
      await response.dispose();
    }
  };
}

export async function artistReviewDownloadAbsence(page: Page, ids: string[]) {
  const results = [];
  for (const id of ids) {
    const response = await page.request.get(
      `/api/v1/recordings/${id}/download`,
      { timeout: 10_000 },
    );
    try {
      const body = await response.body();
      if (body.length > 4096)
        throw new Error("Artist review download absence exceeded its bound");
      const expected = {
        error: "Resource not found",
      } satisfies Schema<"ErrorResponse">;
      expect(
        response.status() === 404 &&
          body.toString() === JSON.stringify(expected),
      ).toBe(true);
      results.push({ status: 404, body: expected });
    } finally {
      await response.dispose();
    }
  }
  return results;
}

export async function artistReviewOriginals() {
  const results = [];
  for (const path of [
    "/workspace/fixtures/e2e-generated/Music/Fixture 440 Hz.wav",
    "/workspace/fixtures/e2e-generated/Archive/Fixture 660 Hz.wav",
  ]) {
    const metadata = await stat(path);
    if (
      !metadata.isFile() ||
      metadata.size > 32 * 1024 * 1024 ||
      metadata.size < 44
    )
      throw new Error(
        "The owned artist review audio fixture exceeded its bound",
      );
    const bytes = await readFile(path);
    if (
      bytes.length !== metadata.size ||
      bytes.subarray(0, 4).toString() !== "RIFF" ||
      bytes.subarray(8, 12).toString() !== "WAVE"
    )
      throw new Error("The owned audio fixture is invalid");
    results.push({
      bytes: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }
  return results;
}
