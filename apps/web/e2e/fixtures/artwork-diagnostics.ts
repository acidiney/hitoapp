import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];
export type Phase =
  | "sign-in"
  | "copies"
  | "approval"
  | "retrieving"
  | "ready"
  | "disconnected";

export function artworkDiagnostics(origin: string, currentPhase: () => Phase) {
  const imports: Schema["ImportProviderPlaylist"][] = [];
  const artworkEnqueues: { phase: Phase; path: string }[] = [];
  const logs: Promise<string>[] = [];
  const urls: string[] = [];
  const imageURLs: string[] = [];
  const downloadMutations: { method: string; url: string }[] = [];
  const errors: string[] = [];
  const consoleErrors: { text: string; url: string }[] = [];
  const failedResponses: {
    owner: string;
    url: string;
    method: string;
    status: number;
    phase: Phase;
  }[] = [];
  const observe = (observed: Page, owner: string) => {
    observed.on("pageerror", (error) => errors.push(error.message));
    observed.on("console", (message) => {
      if (message.type() === "error")
        consoleErrors.push({
          text: message.text(),
          url: message.location().url,
        });
      logs.push(
        Promise.all(message.args().map((argument) => argument.jsonValue()))
          .then((values) => JSON.stringify(values))
          .catch(() => message.text()),
      );
    });
    observed.on("request", (request) => {
      urls.push(request.url());
      const url = new URL(request.url());
      if (
        url.origin === origin &&
        request.method() !== "GET" &&
        (/^\/api\/v1\/(?:recordings|playlists)\/[^/]+\/download$/.test(
          url.pathname,
        ) ||
          /^\/api\/v1\/downloads(?:\/|$)/.test(url.pathname))
      )
        downloadMutations.push({
          method: request.method(),
          url: request.url(),
        });
      if (request.resourceType() === "image") imageURLs.push(request.url());
      if (
        request.method() === "POST" &&
        /\/plugins\/[^/]+\/playlists\/import$/.test(
          new URL(request.url()).pathname,
        )
      )
        imports.push(request.postDataJSON());
      if (
        request.method() === "POST" &&
        /^\/api\/v1\/recordings\/[^/]+\/artwork\/fetch$/.test(
          new URL(request.url()).pathname,
        )
      )
        artworkEnqueues.push({
          phase: currentPhase(),
          path: new URL(request.url()).pathname,
        });
    });
    observed.on("response", (response) => {
      const url = new URL(response.url());
      if (url.origin !== origin) return;
      const at = currentPhase();
      if (response.status() >= 400)
        failedResponses.push({
          owner,
          url: response.url(),
          method: response.request().method(),
          status: response.status(),
          phase: at,
        });
    });
  };

  async function assertPrivacy(
    page: Page,
    providerAddress: string,
    privateValues: string[],
  ) {
    expect(
      urls.filter(
        (url) => new URL(url).origin === new URL(providerAddress).origin,
      ),
    ).toEqual([]);
    expect(imageURLs.length).toBeGreaterThan(0);
    for (const url of imageURLs) expect(new URL(url).origin).toBe(origin);
    expect(urls.some((url) => /\/audio(?:\?|$)/.test(url))).toBe(false);
    const storage = await page.evaluate(() =>
      JSON.stringify({
        local: Object.entries(localStorage),
        session: Object.entries(sessionStorage),
      }),
    );
    const browserLogs = (await Promise.all(logs)).join("\n");
    for (const value of privateValues) {
      expect(browserLogs).not.toContain(value);
      expect(storage).not.toContain(value);
      for (const url of urls) {
        expect(url).not.toContain(value);
        expect(url).not.toContain(encodeURIComponent(value));
      }
    }
  }

  function assertFailures(
    artworkPath: string,
    baseURL: string,
    copies: readonly string[],
  ) {
    expect(errors).toEqual([]);
    expect(downloadMutations).toEqual([]);
    const artworkURL = new URL(artworkPath, baseURL).href;
    const downloadStatusURLs = new Set(
      copies.map(
        (copy) =>
          new URL(
            `/api/v1/playlists/${encodeURIComponent(copy)}/download`,
            baseURL,
          ).href,
      ),
    );
    const otherAbsence = new RegExp(
      `^${new URL("/api/v1/", baseURL).href}(recordings|playlists)/[^/]+/(artwork|download)$`,
    );
    const playlistCoverURLs = new Set(
      copies.map(
        (copy) =>
          new URL(
            `/api/v1/playlists/${encodeURIComponent(copy)}/artwork`,
            baseURL,
          ).href,
      ),
    );
    const messages = new Map([
      [
        401,
        "Failed to load resource: the server responded with a status of 401 (Unauthorized)",
      ],
      [
        404,
        "Failed to load resource: the server responded with a status of 404 (Not Found)",
      ],
    ]);
    for (const response of failedResponses) {
      expect(response.method).toBe("GET");
      if (response.url === new URL("/api/v1/me", baseURL).href) {
        expect(response.status).toBe(401);
        expect(response.phase).toBe("sign-in");
      } else {
        expect(response.owner).toBe("owner");
        expect(response.status).toBe(404);
        if (downloadStatusURLs.has(response.url))
          expect(["copies", "retrieving", "ready", "disconnected"]).toContain(
            response.phase,
          );
        else if (playlistCoverURLs.has(response.url))
          expect(["copies", "retrieving", "ready"]).toContain(response.phase);
        else if (response.url === artworkURL)
          expect(["copies", "retrieving"]).toContain(response.phase);
        else expect(response.url).toMatch(otherAbsence);
      }
    }
    for (const [owner, rejections] of [
      ["owner", 0],
      ["other", 1],
    ] as const)
      expect(
        failedResponses.filter(
          (response) => response.owner === owner && response.status === 401,
        ),
      ).toHaveLength(rejections);
    expect(
      failedResponses.some(
        (response) =>
          response.url === artworkURL &&
          response.phase === "retrieving" &&
          response.status === 404,
      ),
    ).toBe(true);
    const correlated = new Set<number>();
    const unexpected = consoleErrors.filter((diagnostic) => {
      const index = failedResponses.findIndex(
        (response, index) =>
          !correlated.has(index) &&
          response.url === diagnostic.url &&
          diagnostic.text === messages.get(response.status),
      );
      if (index < 0) return true;
      correlated.add(index);
      return false;
    });
    expect(unexpected).toEqual([]);
    expect(correlated.size).toBe(failedResponses.length);
  }

  return { imports, artworkEnqueues, observe, assertPrivacy, assertFailures };
}
