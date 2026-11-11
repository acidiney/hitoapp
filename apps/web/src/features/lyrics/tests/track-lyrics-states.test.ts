import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  candidate,
  review,
  hostile,
  identities,
  observationIdentity,
  plainDocument,
  readyV1,
  readyV2,
  reviewedIdentity,
  serviceA,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  json,
  lyricServer,
  openChange,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

const chosen = reviewedIdentity("cccccccc-0000-4000-8000-000000000001", 2);

async function showLyrics(read: () => Response) {
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(chosen)),
    read,
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  return { lyrics, server };
}

test("plain v2 lyrics keep blank stanza lines as static text and show provider attribution with an empty creator list", async () => {
  const { lyrics } = await showLyrics(() =>
    json(
      readyV2(
        chosen,
        plainDocument(["One", "", "Two"], {
          provider_label: "Open lyrics",
          content_url: "https://lyrics.example/track/1",
          creators: [],
        }),
      ),
    ),
  );
  const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
  expect([...lines.querySelectorAll("li")].map((li) => li.textContent)).toEqual(
    ["One", "", "Two"],
  );
  const link = lyrics.getByRole("link", { name: "Open lyrics" });
  expect(link.getAttribute("href")).toBe("https://lyrics.example/track/1");
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  expect(lyrics.queryByRole("list", { name: "Creators" })).toBeNull();
  expect(lyrics.getByText(/^Not synced · from/)).toBeTruthy();
});

test("anchored v2 lyrics render as static text without times, highlighting or fabricated blank lines", async () => {
  const { lyrics } = await showLyrics(() =>
    json(
      readyV2(chosen, {
        id: "doc",
        rendition_id: "r",
        content: {
          kind: "anchored",
          lines: [
            { start_ms: 0, text: "" },
            { start_ms: 1000, text: "Line A" },
            { start_ms: 1000, text: "Line B" },
            { start_ms: 4000, text: "" },
            { start_ms: 4000, text: "" },
            { start_ms: 5000, text: "Line C" },
            { start_ms: 9000, text: "" },
          ],
        },
      }),
    ),
  );
  const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
  expect([...lines.querySelectorAll("li")].map((li) => li.textContent)).toEqual(
    ["Line A", "Line B", "", "Line C"],
  );
  expect(lines.querySelector("[data-time]")).toBeNull();
  expect(lines.getAttribute("data-synced")).toBe("false");
});

test("v1 timed lyrics keep their existing synced rendering and creators", async () => {
  const { lyrics } = await showLyrics(() =>
    json(
      readyV1(chosen, {
        id: "l",
        rendition_id: "r",
        language: "en",
        lines: [
          { start_ms: 0, end_ms: 1000, text: "Early" },
          { start_ms: 1000, end_ms: 2000, text: "Later" },
        ],
        attribution: {
          creators: ["Writer"],
          provider_label: "Old provider",
          content_url: "https://old.example/l",
        },
      }),
    ),
  );
  const lines = await lyrics.findByRole("group", { name: "Lyric lines" });
  expect(lines.hasAttribute("data-synced")).toBe(true);
  expect(
    [...lines.querySelectorAll("button")].map((line) => line.dataset.state),
  ).toEqual(["next", "next"]);
  expect(lyrics.getByText(/^Synced · from/)).toBeTruthy();
  expect(
    within(lyrics.getByRole("list", { name: "Creators" })).getByText("Writer"),
  ).toBeTruthy();
});

test("hostile lyric, label and attribution text is escaped and never becomes HTML or an unsafe link", async () => {
  const { lyrics } = await showLyrics(() =>
    json(
      readyV2(chosen, {
        ...plainDocument([hostile], {
          provider_label: hostile,
          content_url: "javascript:alert(1)",
        }),
        rendition_label: hostile,
      }),
    ),
  );
  const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
  expect(lines.textContent).toBe(hostile);
  expect(lines.querySelector("img, script")).toBeNull();
  expect(document.querySelector("script")).toBeNull();
  expect(document.querySelector('img[src="x"]')).toBeNull();
  expect(lyrics.queryByRole("link", { name: hostile })).toBeNull();
  expect(lyrics.getAllByText(hostile).length).toBeGreaterThan(0);
});

test("a ready document with null lyrics is Not found and keeps the chosen identity, unlike a temporary failure", async () => {
  const { lyrics, server } = await showLyrics(() =>
    json(readyV2(chosen, null)),
  );
  expect(await lyrics.findByText("None found")).toBeTruthy();
  expect(lyrics.queryByText(/Couldn't check/)).toBeNull();
  expect(lyrics.getByRole("button", { name: "Look up again" })).toBeTruthy();
  expect(
    lyrics.getByRole("button", { name: "Try another plugin" }),
  ).toBeTruthy();
  expect(server.of("read")).toHaveLength(1);
});

test("unsupported, unavailable and upstream errors each get their own message and an explicit retry", async () => {
  let next: () => Response = () => json({ status: "unsupported" });
  const { lyrics, server } = await showLyrics(() => next());
  expect(
    await lyrics.findByText(/can't supply lyrics in a format Hito can read/),
  ).toBeTruthy();
  next = () => json({ status: "unavailable" });
  fireEvent.click(lyrics.getByRole("button", { name: "Look up again" }));
  const alert = await lyrics.findByRole("alert");
  expect(alert.textContent).toContain("Couldn't check");
  expect(lyrics.queryByText("None found")).toBeNull();
  next = () => json({ error: "Upstream is down" }, 502);
  fireEvent.click(lyrics.getByRole("button", { name: "Retry" }));
  await waitFor(() =>
    expect(lyrics.getByRole("alert").textContent).toContain(
      "Open lyrics didn't answer",
    ),
  );
  expect(lyrics.queryByText("None found")).toBeNull();
  next = () => json(readyV2(chosen, plainDocument(["Back again"])));
  fireEvent.click(lyrics.getByRole("button", { name: "Retry" }));
  expect(await lyrics.findByText("Back again")).toBeTruthy();
  expect(server.of("read")).toHaveLength(4);
});

test("a cancelled status ends the pending state without a failure or any lyric text", async () => {
  const { lyrics } = await showLyrics(() => json({ status: "cancelled" }));
  await waitFor(() => expect(lyrics.queryByText("Loading lyrics…")).toBeNull());
  expect(lyrics.queryByRole("alert")).toBeNull();
  expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
  expect(lyrics.getByRole("button", { name: "Show lyrics" })).toBeTruthy();
});

test("a ready reply for another selector, recording or provider reference is rejected as changed and the choices reload", async () => {
  const other = reviewedIdentity("dddddddd-0000-4000-8000-000000000001", 9);
  let listings = 0;
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => {
      listings += 1;
      return json(identities(chosen));
    },
    read: () => json(readyV2(other, plainDocument(["Wrong identity text"]))),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  expect((await lyrics.findByRole("alert")).textContent).toContain(
    "Reload the lyric choices",
  );
  expect(lyrics.queryByText("Wrong identity text")).toBeNull();
  await waitFor(() => expect(listings).toBe(2));
  expect(server.of("read")).toHaveLength(1);
});

test("404 from a read clears the stale selection and reloads the authorized local choices", async () => {
  let listings = 0;
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => {
      listings += 1;
      return json(listings === 1 ? identities(chosen) : identities());
    },
    read: () => json({ error: "Not found" }, 404),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  await waitFor(() =>
    expect(lyrics.queryByRole("radio", { name: /Open lyrics/ })).toBeNull(),
  );
  expect(lyrics.queryByRole("button", { name: "Show lyrics" })).toBeNull();
});

test.each([401, 403, 423])(
  "%i retires private work, clears lyric output and shows the sign-in message instead of choices",
  async (status) => {
    let reads = 0;
    lyricServer({
      services: () => json(services(serviceA)),
      identities: () => json(identities(chosen, observationIdentity)),
      read: () => {
        reads += 1;
        return reads === 1
          ? json(readyV2(chosen, plainDocument(["Private words"])))
          : json({ error: "no" }, status);
      },
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
    await lyrics.findByText("Private words");
    const change = await openChange(lyrics);
    fireEvent.click(change.getByRole("radio", { name: /Second lyrics/ }));
    fireEvent.click(change.getByRole("button", { name: "Done" }));
    expect(lyrics.queryByText("Private words")).toBeNull();
    fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
    expect((await lyrics.findByRole("alert")).textContent).toContain(
      "Sign in again",
    );
    expect(lyrics.queryByText("Private words")).toBeNull();
    expect(lyrics.queryByRole("radio")).toBeNull();
    expect(lyrics.queryByRole("button", { name: "Find lyrics" })).toBeNull();
    expect(document.body.textContent).not.toContain("Private words");
  },
);

test("removal sends the exact identity UUID and revision, clears the read, and a re-added identity has a fresh UUID that is the one read", async () => {
  const first = reviewedIdentity("eeeeeeee-0000-4000-8000-000000000001", 5);
  const readded = reviewedIdentity("eeeeeeee-0000-4000-8000-000000000002", 1);
  let present = [first];
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(...present)),
    remove: () => new Response(null, { status: 204 }),
    candidates: () => json({ items: [candidate("again", "Again hit")] }),
    review: () => json(review("again")),
    retain: () => {
      present = [readded];
      return json(readded);
    },
    read: (call) =>
      json(
        readyV2(
          present[0],
          plainDocument([
            `Text for ${JSON.stringify((call.body as { selector: unknown }).selector)}`,
          ]),
        ),
      ),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  await lyrics.findByText(/Text for/);
  present = [];
  const change = await openChange(lyrics);
  fireEvent.click(change.getByRole("button", { name: "Remove Open lyrics" }));
  await lyrics.findByRole("button", { name: "Find lyrics" });
  const removal = server.of("remove")[0];
  expect(removal.method).toBe("DELETE");
  expect(removal.path).toBe(
    `/api/v1/recordings/${lyricRecordingPage.detail.recording_id}/lyrics/identities/eeeeeeee-0000-4000-8000-000000000001?revision=5`,
  );
  expect(removal.body).toBeUndefined();
  expect(lyrics.queryByText(/Text for/)).toBeNull();
  fireEvent.click(lyrics.getByRole("button", { name: "Find lyrics" }));
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  fireEvent.click(
    await lyrics.findByRole("button", { name: "Review lyrics for Again hit" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Use these lyrics" }),
  );
  await waitFor(() =>
    expect(lyrics.getByRole("button", { name: "Change" })).toBeTruthy(),
  );
  fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
  await lyrics.findByText(/eeeeeeee-0000-4000-8000-000000000002/);
  expect(server.of("read")[1].body).toMatchObject({
    selector: { identity_id: "eeeeeeee-0000-4000-8000-000000000002" },
  });
});

test("eight saved choices, including unavailable ones, explain the limit and offer removal instead of dropping any", async () => {
  const eight = Array.from({ length: 8 }, (_, index) =>
    reviewedIdentity(`ffffffff-0000-4000-8000-00000000000${index}`, 1, {
      selectable: index !== 3,
    }),
  );
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(...eight)),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  const change = await openChange(lyrics);
  await change.findAllByRole("radio");
  expect(change.getAllByRole("radio")).toHaveLength(8);
  expect(change.getByRole("note").textContent).toContain("up to 8");
  expect(
    change.getAllByRole("button", { name: /^Remove Open lyrics/ }),
  ).toHaveLength(8);
  expect(change.getByRole("radio", { name: /Open lyrics \(1\)/ })).toBeTruthy();
});
