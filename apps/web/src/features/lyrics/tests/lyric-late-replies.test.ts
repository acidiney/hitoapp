import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useRecordingLyrics } from "../hooks/useRecordingLyrics";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  candidate,
  identities,
  plainDocument,
  readyV2,
  review,
  reviewedIdentity,
  serviceA,
  serviceB,
  services,
} from "../fixtures/lyricIdentityWire";
import { held, json, lyricServer } from "../fixtures/lyricServer";

afterEach(() => vi.restoreAllMocks());

const detail = lyricRecordingPage.detail;
const chosen = reviewedIdentity("11110000-0000-4000-8000-000000000001", 3);
const other = reviewedIdentity("11110000-0000-4000-8000-000000000002", 1);

function mount(account = "account-1", recording = detail) {
  return renderHook(
    (props: { account: string; recording: typeof detail }) =>
      useRecordingLyrics(props.account, props.recording, false, true),
    { initialProps: { account, recording } },
  );
}

async function ready(result: {
  current: ReturnType<typeof useRecordingLyrics>;
}) {
  await waitFor(() => expect(result.current.identities.loaded).toBe(true));
}

test("a held read is dropped when the selection changes and an immediate explicit read succeeds", async () => {
  const hold = held();
  let reads = 0;
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(chosen, other)),
    read: () => {
      reads += 1;
      return reads === 1
        ? hold.reply
        : json(readyV2(other, plainDocument(["Fresh text"])));
    },
  });
  const { result } = mount();
  await ready(result);
  act(() => result.current.reader.load());
  await waitFor(() => expect(result.current.reader.pending).toBe(true));
  act(() =>
    result.current.identities.select(
      `reviewed:${other.selector.kind === "reviewed" ? other.selector.identity_id : ""}:1`,
    ),
  );
  expect(result.current.reader.pending).toBe(false);
  hold.release(json(readyV2(chosen, plainDocument(["Stale text"]))));
  await act(async () => {
    await Promise.resolve();
  });
  expect(result.current.reader.outcome).toBeNull();
  act(() => result.current.reader.load());
  await waitFor(() =>
    expect(result.current.reader.outcome?.kind).toBe("lyrics"),
  );
  expect(JSON.stringify(result.current.reader.outcome)).toContain("Fresh text");
  expect(JSON.stringify(result.current.reader.outcome)).not.toContain("Stale");
});

test("switching account retires a held read, drops its text and choices, and reloads for the new account only", async () => {
  const hold = held();
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: (call) =>
      json(
        identities(
          call.headers.get("x-hito-account") === "account-2" ? other : chosen,
        ),
      ),
    read: () => hold.reply,
  });
  const { result, rerender } = mount("account-1");
  await ready(result);
  act(() => result.current.reader.load());
  await waitFor(() => expect(result.current.reader.pending).toBe(true));
  rerender({ account: "account-2", recording: detail });
  expect(result.current.identities.items).toBeNull();
  expect(result.current.reader.pending).toBe(false);
  hold.release(json(readyV2(chosen, plainDocument(["Account one text"]))));
  await waitFor(() =>
    expect(result.current.identities.selected?.provider_recording.id).toBe(
      other.provider_recording.id,
    ),
  );
  expect(result.current.reader.outcome).toBeNull();
  expect(JSON.stringify(result.current)).not.toContain("Account one text");
  const reads = server.of("read");
  expect(reads[0].signal?.aborted).toBe(true);
});

test("switching recording retires a held search and review, and a held accept never publishes its stamp or selection", async () => {
  const searchHold = held();
  const acceptHold = held();
  let retains = 0;
  const server = lyricServer({
    services: () => json(services(serviceA, serviceB)),
    identities: () => json(identities()),
    candidates: () => searchHold.reply,
    review: () => json(review("rec-1", "stamp-one")),
    retain: () => {
      retains += 1;
      return acceptHold.reply;
    },
  });
  const { result, rerender } = mount();
  await ready(result);
  await act(async () => {
    void result.current.finder.search.search("held query");
  });
  expect(result.current.finder.search.pending).toBe(true);
  act(() => result.current.finder.chooseService(serviceB.installation_id));
  expect(result.current.finder.search.pending).toBe(false);
  searchHold.release(json({ items: [candidate("late")] }));
  await act(async () => {
    await Promise.resolve();
  });
  expect(result.current.finder.search.candidates).toBeNull();
  await act(async () => {
    await result.current.finder.review.choose(candidate("rec-1"));
  });
  expect(result.current.finder.review.phase).toBe("reviewed");
  await act(async () => {
    void result.current.finder.accept();
  });
  expect(retains).toBe(1);
  const nextRecording = {
    ...detail,
    recording_id: "00000000-0000-4000-8000-0000000000aa",
  };
  rerender({ account: "account-1", recording: nextRecording });
  expect(result.current.finder.review.review).toBeNull();
  expect(result.current.finder.review.phase).toBe("idle");
  acceptHold.release(json(chosen));
  await act(async () => {
    await Promise.resolve();
  });
  expect(result.current.identities.selected).toBeNull();
  expect(result.current.finder.notice).toBeNull();
  expect(server.of("retain")[0].signal?.aborted).toBe(true);
});

test("unmounting aborts every pending lyric request", async () => {
  const hold = held();
  const server = lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(chosen)),
    read: () => hold.reply,
  });
  const { result, unmount } = mount();
  await ready(result);
  act(() => result.current.reader.load());
  await waitFor(() => expect(server.of("read")).toHaveLength(1));
  unmount();
  expect(server.of("read")[0].signal?.aborted).toBe(true);
  hold.release(json(readyV2(chosen, plainDocument(["Never shown"]))));
});

test("explicit cancel ends a pending read without a failure and an immediate retry works", async () => {
  const hold = held();
  let reads = 0;
  lyricServer({
    services: () => json(services(serviceA)),
    identities: () => json(identities(chosen)),
    read: () => {
      reads += 1;
      return reads === 1
        ? hold.reply
        : json(readyV2(chosen, plainDocument(["After cancel"])));
    },
  });
  const { result } = mount();
  await ready(result);
  act(() => result.current.reader.load());
  await waitFor(() => expect(result.current.reader.pending).toBe(true));
  act(() => result.current.reader.cancel());
  expect(result.current.reader.pending).toBe(false);
  expect(result.current.reader.failure).toBeNull();
  hold.release(json(readyV2(chosen, plainDocument(["Late"]))));
  act(() => result.current.reader.load());
  await waitFor(() =>
    expect(result.current.reader.outcome?.kind).toBe("lyrics"),
  );
  expect(JSON.stringify(result.current.reader.outcome)).toContain(
    "After cancel",
  );
});

test("a read that outlives its single fifteen second deadline fails once and is aborted", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    const hold = held();
    const server = lyricServer({
      services: () => json(services(serviceA)),
      identities: () => json(identities(chosen)),
      read: () => hold.reply,
    });
    const { result } = mount();
    await vi.waitFor(() => expect(result.current.identities.loaded).toBe(true));
    act(() => result.current.reader.load());
    await vi.waitFor(() => expect(server.of("read")).toHaveLength(1));
    await act(async () => {
      vi.advanceTimersByTime(15_000);
    });
    expect(result.current.reader.failure?.kind).toBe("deadline");
    expect(server.of("read")[0].signal?.aborted).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});
