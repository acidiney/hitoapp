import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { forgetAutomaticLookups } from "../api/automaticLyrics";
import { useRecordingLyrics } from "../hooks/useRecordingLyrics";
import { setAutomaticLyrics } from "../fixtures/automaticLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  automaticReadyV2,
  identities,
  observationIdentity,
  plainDocument,
  readyV2,
  reviewedIdentity,
  serviceA,
  serviceB,
  services,
} from "../fixtures/lyricIdentityWire";
import {
  held,
  json,
  lyricServer,
  openLyricsTab,
  renderLyricsPage,
} from "../fixtures/lyricServer";
import { invalidate } from "../../../shared/invalidation";
import {
  connectionNeedsCredentials,
  resetCredentialRequirements,
  useCredentialRequirements,
} from "../../plugins/model/credentialRequirements";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  forgetAutomaticLookups();
  resetCredentialRequirements();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

const detail = lyricRecordingPage.detail;
const chosen = reviewedIdentity("cccccccc-0000-4000-8000-000000000001", 2);
const stampA = {
  installation_id: serviceA.installation_id,
  installation_revision: serviceA.installation_revision,
};
const credentialReply = (stamp?: unknown) =>
  json(
    {
      error: "This music service connection needs its credentials updated.",
      code: "plugin_credentials_required",
      ...(stamp === undefined ? {} : { plugin_connection: stamp }),
    },
    409,
  );

function mount(account = "account-1", automatic = false) {
  return renderHook(
    (props: { account: string }) => ({
      lyrics: useRecordingLyrics(props.account, detail, automatic, true),
      flags: useCredentialRequirements("account-1"),
    }),
    { initialProps: { account } },
  );
}

type Mounted = ReturnType<typeof mount>["result"];

const ready = (result: Mounted) =>
  waitFor(() => expect(result.current.lyrics.identities.loaded).toBe(true));

const plugin = (revision: number) => ({
  id: serviceA.installation_id,
  revision,
  health: undefined,
});

function readServer(read: () => Response | Promise<Response>) {
  return lyricServer({
    services: () => json(services(serviceA, serviceB)),
    identities: () => json(identities(chosen, observationIdentity)),
    read,
  });
}

test("the credential 409 marks the stamped connection only, keeps the identity and never asks for sign-in", async () => {
  readServer(() => credentialReply(stampA));
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  const { lyrics, flags } = result.current;
  expect(lyrics.reader.failure?.message).toBe(
    "This music service connection needs its credentials updated.",
  );
  expect(lyrics.lost).toBe(false);
  expect(lyrics.identities.selected?.provider_recording.id).toBe(
    chosen.provider_recording.id,
  );
  expect(lyrics.identities.items).toHaveLength(2);
  expect(connectionNeedsCredentials(flags, plugin(7))).toBe(true);
  expect(
    connectionNeedsCredentials(flags, {
      id: serviceB.installation_id,
      revision: serviceB.installation_revision,
      health: undefined,
    }),
  ).toBe(false);
});

test("a listener 401, an inaccessible 404 and an unrelated 409 do not mark any connection", async () => {
  const replies = [
    () =>
      json(
        { error: "no", code: "plugin_credentials_required", ...stampA },
        401,
      ),
    () => json({ error: "Not found", plugin_connection: stampA }, 404),
    () =>
      json({ error: "Changed", code: "stale", plugin_connection: stampA }, 409),
  ];
  const expected = ["lost", "gone", "conflict"];
  const seen: string[] = [];
  for (const reply of replies) {
    const server = readServer(reply);
    const { result, unmount } = mount();
    await ready(result);
    act(() => result.current.lyrics.reader.load());
    await waitFor(() => expect(server.of("read")).toHaveLength(1));
    await waitFor(() =>
      expect(
        result.current.lyrics.lost ||
          result.current.lyrics.reader.failure !== null ||
          result.current.lyrics.identities.items === null ||
          result.current.lyrics.identities.selected === null,
      ).toBe(true),
    );
    seen.push(
      result.current.lyrics.lost
        ? "lost"
        : (result.current.lyrics.reader.failure?.kind ?? "none"),
    );
    expect(result.current.flags.size).toBe(0);
    unmount();
    vi.unstubAllGlobals();
  }
  expect(seen).toEqual(expected);
});

test("an error without a stamp shows generic guidance and marks nothing", async () => {
  readServer(() => credentialReply());
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  expect(result.current.flags.size).toBe(0);
  expect(result.current.lyrics.lost).toBe(false);
});

test("a stamp for an older revision does not apply to the changed connection", async () => {
  readServer(() => credentialReply({ ...stampA, installation_revision: 6 }));
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  expect(connectionNeedsCredentials(result.current.flags, plugin(7))).toBe(
    false,
  );
});

test("a late credential error after an account switch is discarded", async () => {
  const hold = held();
  const server = readServer(() => hold.reply);
  const { result, rerender } = mount("account-1");
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() => expect(result.current.lyrics.reader.pending).toBe(true));
  rerender({ account: "account-2" });
  hold.release(credentialReply(stampA));
  await act(async () => {
    await Promise.resolve();
  });
  await waitFor(() =>
    expect(result.current.lyrics.identities.loaded).toBe(true),
  );
  expect(server.of("read")[0].signal?.aborted).toBe(true);
  expect(result.current.flags.size).toBe(0);
  expect(result.current.lyrics.reader.failure).toBeNull();
});

test("after a credential update the failed read is retried once under its normal lifecycle", async () => {
  let reads = 0;
  const server = readServer(() => {
    reads += 1;
    return reads === 1
      ? credentialReply(stampA)
      : json(readyV2(chosen, plainDocument(["Back again"])));
  });
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(server.of("read")).toHaveLength(1);
  act(() => {
    invalidate("plugins", serviceA.installation_id);
    invalidate("lyrics");
  });
  await waitFor(() =>
    expect(result.current.lyrics.reader.outcome?.kind).toBe("lyrics"),
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
  expect(server.of("read")).toHaveLength(2);
  expect(result.current.lyrics.reader.failure).toBeNull();
});

test("a repeated credential error after the update waits for the next explicit action", async () => {
  const server = readServer(() => credentialReply(stampA));
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  act(() => invalidate("plugins", serviceA.installation_id));
  await waitFor(() => expect(server.of("read")).toHaveLength(2));
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
  expect(server.of("read")).toHaveLength(2);
});

test("no retry runs when the choice is no longer selectable after the update", async () => {
  let listings = 0;
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => {
      listings += 1;
      return json(
        listings === 1
          ? identities(chosen)
          : identities({ ...chosen, selectable: false }),
      );
    },
    read: () => credentialReply(stampA),
  });
  const { result } = mount();
  await ready(result);
  act(() => result.current.lyrics.reader.load());
  await waitFor(() =>
    expect(result.current.lyrics.reader.failure?.kind).toBe("credentials"),
  );
  act(() => invalidate("plugins", serviceA.installation_id));
  await waitFor(() => expect(listings).toBe(2));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
  expect(server.of("read")).toHaveLength(1);
});

test("an automatic lookup that needs credentials is retried once after the update", async () => {
  let lookups = 0;
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities()),
    automatic: () => {
      lookups += 1;
      return lookups === 1
        ? credentialReply(stampA)
        : json(automaticReadyV2(plainDocument(["Found text"])));
    },
  });
  const { result } = mount("account-1", true);
  await waitFor(() =>
    expect(result.current.lyrics.lookup.failure?.kind).toBe("credentials"),
  );
  expect(connectionNeedsCredentials(result.current.flags, plugin(7))).toBe(
    true,
  );
  act(() => invalidate("plugins", serviceA.installation_id));
  await waitFor(() =>
    expect(result.current.lyrics.lookup.outcome?.kind).toBe("lyrics"),
  );
  expect(server.of("automatic")).toHaveLength(2);
});

test("the lyric card explains the credential problem with a plugin settings link while audio and manual choices stay untouched", async () => {
  const server = readServer(() => credentialReply(stampA));
  const { play } = renderLyricsPage();
  const lyrics = openLyricsTab();
  fireEvent.click(await lyrics.findByRole("button", { name: "Show lyrics" }));
  const alert = await lyrics.findByRole("alert");
  expect(alert.textContent).toContain("needs its credentials updated");
  expect(document.body.textContent).not.toContain("Sign in again");
  expect(
    lyrics
      .getByRole("link", { name: "Open plugin settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(play).not.toHaveBeenCalled();
  expect(server.of("audio")).toHaveLength(0);
  fireEvent.click(lyrics.getByRole("button", { name: "Try another plugin" }));
  const change = within(
    await screen.findByRole("dialog", { name: "Lyrics for this track" }),
  );
  const second = change.getByRole("radio", { name: /Second lyrics/ });
  fireEvent.click(second);
  fireEvent.click(change.getByRole("button", { name: "Done" }));
  expect(
    await lyrics.findByRole("button", { name: "Show lyrics" }),
  ).toBeTruthy();
  expect(screen.queryByText("Sign in again")).toBeNull();
  expect(server.of("audio")).toHaveLength(0);
  expect(play).not.toHaveBeenCalled();
});
