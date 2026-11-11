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
  identities,
  observationIdentity,
  plainDocument,
  readyV2,
  review,
  reviewedIdentity,
  serviceA,
  serviceB,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  json,
  lyricServer,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";
import { openTrackEditors } from "../../recordings/fixtures/trackEditors";

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

const saved = reviewedIdentity("aaaaaaaa-0000-4000-8000-000000000001");

test("opening Lyrics lists saved choices and services locally without any search or read, and choosing a service sends nothing", async () => {
  const server = lyricServer({
    services: () => json(services(serviceA, serviceB)),
    identities: () => json(identities(saved, observationIdentity)),
  });
  renderLyricsPage();
  expect(server.of("services")).toHaveLength(0);
  const lyrics = openLyricsTab();
  await lyrics.findByRole("radio", { name: /Open lyrics/ });
  expect(server.of("services")).toHaveLength(1);
  expect(server.of("identities")).toHaveLength(1);
  fireEvent.click(lyrics.getByRole("button", { name: "Find lyrics" }));
  fireEvent.click(lyrics.getByRole("combobox", { name: "Lyrics service" }));
  fireEvent.click(await screen.findByRole("option", { name: "Second lyrics" }));
  const search = lyrics.getByRole("searchbox", { name: "Search text" });
  fireEvent.change(search, { target: { value: "typed text" } });
  expect(
    server.calls.map((call) => call.route).sort(),
    "No candidate search, review or read may happen until the user asks",
  ).toEqual(["identities", "services"]);
  for (const call of server.calls) {
    expect(call.headers.get("x-hito-account")).toBe(lyricRecordingPage.user.id);
    expect(call.headers.get("x-hito-request")).toBe("1");
  }
});

test("the search text is seeded from the committed title and artist and stays editable", async () => {
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Find lyrics" }));
  const { title, artist } = lyricRecordingPage.detail;
  const field = lyrics.getByRole<HTMLInputElement>("searchbox", {
    name: "Search text",
  });
  expect(field.value).toBe(`${title.value} ${artist.value}`);
  fireEvent.change(field, { target: { value: "edited query" } });
  expect(field.value).toBe("edited query");
});

test("saved choices show their service label, keep unavailable ones disabled with Remove, and observations cannot be removed", async () => {
  const unavailable = reviewedIdentity(
    "aaaaaaaa-0000-4000-8000-000000000002",
    4,
    {
      selectable: false,
      label: "Gone lyrics",
    },
  );
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(saved, unavailable, observationIdentity)),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  const gone = await lyrics.findByRole<HTMLInputElement>("radio", {
    name: /Gone lyrics/,
  });
  expect(gone.disabled).toBe(true);
  expect(
    lyrics
      .getByRole("button", { name: "Remove Gone lyrics" })
      .hasAttribute("disabled"),
  ).toBe(false);
  expect(
    lyrics.getByRole("button", { name: "Remove Open lyrics" }),
  ).toBeTruthy();
  expect(
    lyrics.queryByRole("button", { name: "Remove Second lyrics" }),
  ).toBeNull();
  expect(lyrics.getByText(/Service unavailable/)).toBeTruthy();
  expect(
    lyrics.getByRole<HTMLInputElement>("radio", { name: /Open lyrics/ })
      .checked,
  ).toBe(true);
});

test("choose service, explicit search, review, accept, local chooser and read use the exact bodies and never touch audio, sources, downloads or metadata", async () => {
  const created = reviewedIdentity("bbbbbbbb-0000-4000-8000-000000000009");
  const existing = [observationIdentity];
  const server = lyricServer({
    services: () => json(services(serviceA, serviceB)),
    identities: () =>
      json(
        identities(
          ...(server.of("retain").length > 0
            ? [...existing, created]
            : existing),
        ),
      ),
    candidates: () => json({ items: [candidate("rec-9", "Found title")] }),
    review: () => json(review("rec-9", "opaque stamp/+=")),
    retain: () => json(created),
    read: () =>
      json(readyV2(created, plainDocument(["First saved line", "", "Second"]))),
  });
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  renderLyricsPage();
  openTrackEditors();
  const draftField = within(
    screen.getByRole("region", { name: "Details" }),
  ).getByRole<HTMLInputElement>("textbox", { name: "Track title" });
  fireEvent.change(draftField, { target: { value: "Unsent draft" } });
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Find lyrics" }));
  fireEvent.click(lyrics.getByRole("combobox", { name: "Lyrics service" }));
  fireEvent.click(await screen.findByRole("option", { name: "Open lyrics" }));
  fireEvent.change(lyrics.getByRole("searchbox", { name: "Search text" }), {
    target: { value: "  my query  " },
  });
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  expect((await lyrics.findAllByRole("listitem")).length).toBeGreaterThan(0);
  expect(server.of("candidates")[0].body).toEqual({
    installation_id: serviceA.installation_id,
    installation_revision: serviceA.installation_revision,
    query: "my query",
  });
  fireEvent.click(
    lyrics.getByRole("button", { name: "Review lyrics for Found title" }),
  );
  const dialog = within(
    await screen.findByRole("dialog", {
      name: "Are these the lyrics you want?",
    }),
  );
  await dialog.findByText("Reviewed album text");
  expect(server.of("review")[0].body).toEqual({
    installation_id: serviceA.installation_id,
    installation_revision: serviceA.installation_revision,
    provider_recording_id: "rec-9",
  });
  expect(server.of("retain")).toHaveLength(0);
  fireEvent.click(dialog.getByRole("button", { name: "Use these lyrics" }));
  await waitFor(() => expect(server.of("retain")).toHaveLength(1));
  expect(server.of("retain")[0].body).toEqual({
    candidate: {
      installation_id: serviceA.installation_id,
      installation_revision: serviceA.installation_revision,
      provider_recording_id: "rec-9",
    },
    review_stamp: "opaque stamp/+=",
  });
  await waitFor(() =>
    expect(
      lyrics.getByRole<HTMLInputElement>("radio", { name: /Open lyrics/ })
        .checked,
    ).toBe(true),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(server.of("read")).toHaveLength(0);
  expect(lyrics.getByRole("status").textContent).toContain("Saved Open lyrics");
  fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
  const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
  expect([...lines.querySelectorAll("li")].map((li) => li.textContent)).toEqual(
    ["First saved line", "", "Second"],
  );
  expect(server.of("read")[0].body).toEqual({
    selector: created.selector,
    installation_revision: created.installation_revision,
  });
  expect(server.of("audio")).toHaveLength(0);
  expect(play).not.toHaveBeenCalled();
  expect(draftField.value).toBe("Unsent draft");
  expect(
    server.calls
      .filter((call) => call.method !== "GET")
      .every((call) => call.path.includes("/lyrics")),
  ).toBe(true);
});

test("with no connected lyric service the finder links to plugin settings and sends no external request", async () => {
  const server = lyricServer({
    services: () => json(services()),
    identities: () => json(identities()),
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Find lyrics" }));
  expect(
    lyrics
      .getByRole("link", { name: "Connect a lyrics service" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(lyrics.queryByRole("searchbox")).toBeNull();
  expect(server.calls.map((call) => call.route).sort()).toEqual([
    "identities",
    "services",
  ]);
});

test("an empty search shows a search-specific empty state and the query stays editable for an explicit retry", async () => {
  let searches = 0;
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
    candidates: () => {
      searches += 1;
      return json({
        items: searches === 1 ? [] : [candidate("x", "Later hit")],
      });
    },
  });
  renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Find lyrics" }));
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  expect(await lyrics.findByText(/No matches/)).toBeTruthy();
  fireEvent.change(lyrics.getByRole("searchbox", { name: "Search text" }), {
    target: { value: "better words" },
  });
  fireEvent.click(lyrics.getByRole("button", { name: "Search" }));
  await lyrics.findByRole("button", { name: "Review lyrics for Later hit" });
  expect(lyrics.queryByText(/No matches/)).toBeNull();
});
