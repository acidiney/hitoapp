import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import {
  compatibleProvider,
  compatibleReady,
  identities,
  plainDocument,
  providers,
  readyV2,
  reviewedIdentity,
  serviceA,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  held,
  json,
  lyricServer,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";

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

const forbidden = ["candidates", "review", "retain"] as const;

function automaticServer(overrides: Parameters<typeof lyricServer>[0] = {}) {
  return lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
    legacy_providers: () => json(providers(compatibleProvider)),
    legacy_read: () =>
      json(
        compatibleReady(compatibleProvider, [
          [0, "Auto one"],
          [2000, "Auto two"],
        ]),
      ),
    ...overrides,
  });
}

describe("automatic lookup on", () => {
  setAutomaticLyrics(true);

  test("a recording without a saved identity looks up lyrics through the compatible path without searching, reviewing or saving", async () => {
    const hold = held();
    const server = automaticServer({ legacy_providers: () => hold.reply });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    const skeleton = await lyrics.findByRole("region", {
      name: "Looking up lyrics…",
    });
    expect(skeleton.getAttribute("aria-busy")).toBe("true");
    hold.release(json(providers(compatibleProvider)));
    expect(await lyrics.findByText("Auto one")).toBeTruthy();
    expect(lyrics.getByText("Synced · from Open lyrics")).toBeTruthy();
    expect(server.of("legacy_providers")).toHaveLength(1);
    expect(server.of("legacy_read")[0].body).toEqual({
      observation_id: compatibleProvider.observation_id,
      installation_revision: compatibleProvider.installation_revision,
    });
    for (const route of forbidden) expect(server.of(route)).toHaveLength(0);
    expect(server.of("read")).toHaveLength(0);
    expect(lyrics.getByRole("button", { name: "Find lyrics" })).toBeTruthy();
  });

  test("no provider lyrics shows None found, and Look up again repeats the lookup", async () => {
    let reads = 0;
    const server = automaticServer({
      legacy_read: () => {
        reads += 1;
        return json(
          compatibleReady(
            compatibleProvider,
            reads === 1 ? null : [[0, "Second try"]],
          ),
        );
      },
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
    fireEvent.click(lyrics.getByRole("button", { name: "Look up again" }));
    expect(await lyrics.findByText("Second try")).toBeTruthy();
    expect(server.of("legacy_providers")).toHaveLength(2);
  });

  test("an empty provider list is None found, not a failure", async () => {
    automaticServer({ legacy_providers: () => json(providers()) });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("None found")).toBeTruthy();
    expect(lyrics.queryByText("Couldn't check")).toBeNull();
  });

  test("a temporary failure is Couldn't check with Retry, never None found", async () => {
    let fail = true;
    automaticServer({
      legacy_read: () =>
        fail
          ? json({ error: "down" }, 502)
          : json(compatibleReady(compatibleProvider, [[0, "Recovered"]])),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Couldn't check")).toBeTruthy();
    expect(lyrics.queryByText("None found")).toBeNull();
    fail = false;
    fireEvent.click(lyrics.getByRole("button", { name: "Retry" }));
    expect(await lyrics.findByText("Recovered")).toBeTruthy();
  });

  test("a saved identity keeps the automatic read of the default selection and skips the compatible path", async () => {
    const saved = reviewedIdentity("88880000-0000-4000-8000-000000000001", 1);
    const server = automaticServer({
      identities: () => json(identities(saved)),
      read: () => json(readyV2(saved, plainDocument(["Saved text"]))),
    });
    renderLyricsPage();
    const lyrics = openLyricsTab();
    expect(await lyrics.findByText("Saved text")).toBeTruthy();
    expect(server.of("legacy_providers")).toHaveLength(0);
    expect(server.of("legacy_read")).toHaveLength(0);
  });
});

describe("automatic lookup off", () => {
  setAutomaticLyrics(false);

  test("nothing is looked up until the user chooses to find lyrics", async () => {
    const server = automaticServer();
    renderLyricsPage();
    const lyrics = openLyricsTab();
    await waitFor(() => expect(server.of("identities")).toHaveLength(1));
    await lyrics.findByRole("button", { name: "Find lyrics" });
    expect(server.of("legacy_providers")).toHaveLength(0);
    expect(server.of("legacy_read")).toHaveLength(0);
    expect(screen.queryByText("Looking up lyrics…")).toBeNull();
  });
});
