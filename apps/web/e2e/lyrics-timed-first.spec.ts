import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  connectLyrics,
  lyricsProvider,
  plainCandidate,
  timedCandidate,
  type LyricsCandidate,
} from "./fixtures/lyrics-provider";
import { signOutFromShell } from "./fixtures/account";
import { trackTitleLink } from "./fixtures/track-row";

type Facts = Pick<LyricsCandidate, "title" | "artist" | "duration_ms">;

async function libraryTrack(page: Page, title: string) {
  const listed = await page.request.get(
    `/api/v1/recordings?q=${encodeURIComponent(title)}`,
  );
  const body = await listed.json();
  const recording = body.items?.find(
    (item: Schema<"Recording">) => item.title === title,
  ) as Schema<"Recording">;
  if (!recording) throw new Error("The scanned fixture library is required");
  const facts: Facts = {
    title: recording.title,
    artist: recording.artist,
    duration_ms: recording.duration_ms!,
  };
  return { id: recording.id, facts };
}

function automaticRequests(page: Page) {
  const seen: { recording: string; body: string | null }[] = [];
  page.on("request", (request) => {
    const match = /\/api\/v1\/recordings\/([^/]+)\/lyrics\/automatic$/.exec(
      new URL(request.url()).pathname,
    );
    if (match && request.method() === "POST")
      seen.push({ recording: match[1], body: request.postData() });
  });
  return seen;
}

async function removeSavedChoices(
  page: Page,
  recording: string,
  origin: string,
) {
  const listed = await page.request.get(
    `/api/v1/recordings/${recording}/lyrics/identities`,
  );
  const identities = (await listed.json())
    .items as Schema<"RecordingLyricIdentity">[];
  for (const identity of identities) {
    if (identity.selector.kind !== "reviewed") continue;
    const removed = await page.request.delete(
      `/api/v1/recordings/${recording}/lyrics/identities/${identity.selector.identity_id}?revision=${identity.selector.identity_revision}`,
      { headers: { "x-hito-request": "1", origin } },
    );
    expect(removed.status()).toBeLessThan(300);
  }
}

async function open(page: Page, recording: string) {
  await page.goto(`/recordings/${recording}?tab=lyrics`);
  return page.getByRole("region", { name: "Lyrics", exact: true });
}

async function withProvider(
  page: Page,
  run: (
    provider: Awaited<ReturnType<typeof lyricsProvider>>,
    connection: Awaited<ReturnType<typeof connectLyrics>>,
  ) => Promise<void>,
  baseURL: string,
  administrator?: APIRequestContext,
) {
  const provider = await lyricsProvider();
  const connection = await connectLyrics(
    page.request,
    new URL(baseURL).origin,
    provider,
    administrator,
  );
  try {
    await run(provider, connection);
    expect(provider.state.unexpected).toBe(0);
  } finally {
    await connection.remove();
    await provider.close();
  }
}

test("a timed result listed after a plain result is the one shown, from one automatic request", async ({
  page,
  baseURL,
}) => {
  const track = await libraryTrack(page, "Fixture 440 Hz");
  const requests = automaticRequests(page);
  await withProvider(
    page,
    async (provider) => {
      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        timedCandidate("timed-b", track.facts),
      ];
      const panel = await open(page, track.id);
      await expect(panel.getByText("Timed river line")).toBeVisible();
      await expect(panel.getByText("Plain river line")).toHaveCount(0);
      await expect(panel.getByText("Lyrics fixture").first()).toBeVisible();
      await expect(panel.getByText("None found")).toHaveCount(0);
      expect(requests).toEqual([{ recording: track.id, body: "{}" }]);
      expect(provider.calls.filter((call) => call === "search")).toHaveLength(
        1,
      );
      expect(provider.reads()).toEqual(["lyrics:plain-a", "lyrics:timed-b"]);
    },
    baseURL!,
  );
});

test("plain lyrics are the fallback only when the whole page holds no timed result", async ({
  page,
  baseURL,
}) => {
  const track = await libraryTrack(page, "Fixture 440 Hz");
  await withProvider(
    page,
    async (provider) => {
      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        { ...timedCandidate("empty-b", track.facts), lyrics: null },
      ];
      const panel = await open(page, track.id);
      await expect(panel.getByText("Plain river line")).toBeVisible();
      await expect(panel.getByText("Not synced")).toBeVisible();
      await expect(panel.getByText("Timed river line")).toHaveCount(0);
      expect(provider.reads()).toEqual(["lyrics:plain-a", "lyrics:empty-b"]);
    },
    baseURL!,
  );
});

test("a later failure is Couldn't check with Retry, never plain lyrics, and Retry can recover", async ({
  page,
  baseURL,
}) => {
  const track = await libraryTrack(page, "Fixture 440 Hz");
  const requests = automaticRequests(page);
  await withProvider(
    page,
    async (provider) => {
      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        { ...timedCandidate("broken-b", track.facts), lyrics: "fail" },
      ];
      const panel = await open(page, track.id);
      await expect(panel.getByText("Couldn't check").first()).toBeVisible();
      await expect(panel.getByText("Plain river line")).toHaveCount(0);
      await expect(panel.getByText("None found")).toHaveCount(0);
      expect(requests).toHaveLength(1);

      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        timedCandidate("broken-b", track.facts),
      ];
      await panel.getByRole("button", { name: "Retry" }).click();
      await expect(panel.getByText("Timed river line")).toBeVisible();
      await expect(panel.getByText("Plain river line")).toHaveCount(0);
      expect(requests).toHaveLength(2);
    },
    baseURL!,
  );
});

test("an incomplete result page is reported as several matches and nothing is chosen for the user", async ({
  page,
  baseURL,
}) => {
  const track = await libraryTrack(page, "Fixture 440 Hz");
  await withProvider(
    page,
    async (provider) => {
      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        timedCandidate("timed-b", track.facts),
      ];
      provider.state.continuation = true;
      const panel = await open(page, track.id);
      await expect(panel.getByText("Several matches")).toBeVisible();
      await expect(panel.getByText("Timed river line")).toHaveCount(0);
      await expect(panel.getByText("Plain river line")).toHaveCount(0);
      expect(provider.reads()).toEqual([]);
    },
    baseURL!,
  );
});

test("a plain lyric choice made by the user stays selected over an automatic timed alternative", async ({
  page,
  baseURL,
}) => {
  const track = await libraryTrack(page, "Fixture 440 Hz");
  const requests = automaticRequests(page);
  await withProvider(
    page,
    async (provider) => {
      const manual = "manual plain";
      provider.state.candidates = [
        plainCandidate("plain-a", track.facts),
        timedCandidate("timed-b", track.facts),
      ];
      provider.state.listing = (query) =>
        query === manual
          ? provider.state.candidates.filter((item) => item.id === "plain-a")
          : provider.state.candidates;
      const panel = await open(page, track.id);
      await expect(panel.getByText("Timed river line")).toBeVisible();
      expect(requests).toHaveLength(1);

      await panel.getByRole("button", { name: "Change" }).click();
      await page.getByRole("searchbox", { name: "Search text" }).fill(manual);
      await page.getByRole("button", { name: "Search", exact: true }).click();
      await page
        .getByRole("list", { name: "Lyric matches" })
        .getByRole("button")
        .click();
      await page.getByRole("button", { name: "Use these lyrics" }).click();
      await expect(
        page.getByRole("status").filter({ hasText: "Saved" }),
      ).toBeVisible();
      await panel.getByRole("button", { name: "Show lyrics" }).click();
      await expect(panel.getByText("Plain river line")).toBeVisible();
      await expect(panel.getByText("Timed river line")).toHaveCount(0);

      await open(page, track.id);
      await expect(panel.getByText("Plain river line")).toBeVisible();
      await expect(panel.getByText("Timed river line")).toHaveCount(0);
      expect(requests).toHaveLength(1);
      await removeSavedChoices(page, track.id, new URL(baseURL!).origin);
    },
    baseURL!,
  );
});

test("changing track cancels the held automatic lookup and the new track looks up on its own", async ({
  page,
  baseURL,
}) => {
  const first = await libraryTrack(page, "Fixture 440 Hz");
  const second = await libraryTrack(page, "Fixture 660 Hz");
  const requests = automaticRequests(page);
  await withProvider(
    page,
    async (provider) => {
      provider.state.candidates = [
        timedCandidate("first-timed", first.facts),
        plainCandidate("second-plain", second.facts),
      ];
      provider.state.listing = (query) =>
        provider.state.candidates.filter((item) => query.includes(item.title));
      provider.hold("first-timed");
      const panel = await open(page, first.id);
      await provider.held("first-timed");
      await expect(
        panel.getByRole("region", { name: "Looking up lyrics…" }),
      ).toBeVisible();

      await page
        .getByRole("link", { name: "Library", exact: true })
        .first()
        .click();
      await trackTitleLink(page.locator("main"), "Fixture 660 Hz").click();
      await expect(page).toHaveURL(new RegExp(`/recordings/${second.id}`));
      await page.getByRole("tab", { name: "Lyrics" }).click();
      await expect(
        page
          .getByRole("region", { name: "Lyrics", exact: true })
          .getByText("Plain river line"),
      ).toBeVisible();
      await expect.poll(() => provider.abandoned).toEqual(["first-timed"]);
      provider.release("first-timed");
      await expect(page.getByText("Timed river line")).toHaveCount(0);
      expect(requests.map((request) => request.recording)).toEqual([
        first.id,
        second.id,
      ]);
    },
    baseURL!,
  );
});

test.describe("account change", () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test("signing out cancels the held automatic lookup and the next account sees none of it", async ({
    page,
    browser,
    baseURL,
  }) => {
    const password = "lyric account change password";
    const origin = new URL(baseURL!).origin;
    const headers = { "x-hito-request": "1", origin };
    const admin = await browser.newContext({ baseURL: baseURL! });
    try {
      await admin.request.post("/api/v1/login", {
        headers,
        data: { username: "admin", password: "docker browser test password" },
      });
      const libraries: Schema<"Library">[] = await (
        await admin.request.get("/api/v1/libraries")
      ).json();
      const shared = libraries.find(
        (library: { personal?: boolean }) => !library.personal,
      )!;
      const names = [
        `lyric-owner-${Date.now() % 1_000_000}`,
        `lyric-next-${Date.now() % 1_000_000}`,
      ];
      for (const username of names) {
        const created = await admin.request.post("/api/v1/users", {
          headers,
          data: { username, password },
        });
        expect(created.status()).toBe(201);
        const member: Schema<"User"> = await created.json();
        const joined = await admin.request.put(
          `/api/v1/libraries/${shared.id}/members/${member.id}`,
          { headers },
        );
        expect(joined.ok()).toBe(true);
      }
      const signIn = async (username: string) => {
        await page.goto("/");
        await page.getByLabel("Username", { exact: true }).fill(username);
        await page.getByLabel("Password", { exact: true }).fill(password);
        await page
          .getByRole("button", { name: "Sign in", exact: true })
          .click();
        await expect(
          page.getByRole("heading", { name: "Your library", exact: true }),
        ).toBeVisible();
      };
      await signIn(names[0]);
      const track = await libraryTrack(page, "Fixture 440 Hz");
      await withProvider(
        page,
        async (provider) => {
          provider.state.candidates = [
            timedCandidate("owner-timed", track.facts),
          ];
          provider.hold("owner-timed");
          const panel = await open(page, track.id);
          await provider.held("owner-timed");
          await expect(
            panel.getByRole("region", { name: "Looking up lyrics…" }),
          ).toBeVisible();

          await signOutFromShell(page);
          await expect(
            page.getByRole("heading", { name: "Sign in", exact: true }),
          ).toBeVisible();
          await expect.poll(() => provider.abandoned).toEqual(["owner-timed"]);

          await signIn(names[1]);
          const next = await open(page, track.id);
          await expect(
            next.getByText("No lyrics service", { exact: true }),
          ).toBeVisible();
          provider.release("owner-timed");
          await expect(page.getByText("Timed river line")).toHaveCount(0);
          expect(provider.reads()).toEqual(["lyrics:owner-timed"]);
        },
        baseURL!,
        admin.request,
      );
    } finally {
      await admin.close();
    }
  });
});
