import { createElement } from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import ArtistGroupPhoto from "../components/ArtistGroupPhoto";
import { artistIdentityGroupDetail } from "../fixtures/artistIdentity";

const MUSICBRAINZ = "0f800000-0000-4000-8000-0000000000a1";
const CREDIT = "0123456789abcdef0123456789abcdef";
const PHOTO = `/api/v1/cover-art/artist/${MUSICBRAINZ}`;

const group: Schema<"ArtistIdentityGroup"> = {
  ...artistIdentityGroupDetail.group,
  provider: { kind: "builtin_musicbrainz" },
  provider_artist_id: MUSICBRAINZ,
  credited_photo_artist_id: undefined,
};

let photoStatus = 404;
let reread: { credited_photo_artist_id?: string } = {};
let calls: { url: string; signal: AbortSignal | null | undefined }[] = [];

beforeEach(() => {
  photoStatus = 404;
  reread = {};
  calls = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, signal: init?.signal });
      if (url.includes("/cover-art/"))
        return new Response("{}", { status: photoStatus });
      return new Response(JSON.stringify({ group: reread }), { status: 200 });
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function mount() {
  return render(createElement(ArtistGroupPhoto, { account: "account", group }));
}

function sources() {
  return Array.from(document.querySelectorAll("img")).map((image) =>
    image.getAttribute("src"),
  );
}

function rereads() {
  return calls.filter((call) => call.url.includes("/artist-identity-groups/"));
}

async function failPhoto() {
  await act(async () => {
    fireEvent.error(document.querySelector("img")!);
  });
}

test("a missing musicbrainz photo re-reads the group once and shows the credited photo it now offers", async () => {
  reread = { credited_photo_artist_id: CREDIT };
  mount();
  expect(sources()).toEqual([PHOTO]);
  await failPhoto();
  await vi.waitFor(() =>
    expect(sources()).toEqual([`/api/v1/cover-art/credited-artist/${CREDIT}`]),
  );
  expect(rereads()).toHaveLength(1);
  expect(rereads()[0].url).toContain(
    `/artist-identity-groups/${encodeURIComponent(group.id)}`,
  );
});

test("a photo that fails with anything but 404 never re-reads the group", async () => {
  for (const status of [202, 502]) {
    photoStatus = status;
    calls = [];
    const view = mount();
    await failPhoto();
    await vi.waitFor(() =>
      expect(calls.some((call) => call.url.includes("/cover-art/"))).toBe(true),
    );
    await act(async () => {});
    expect(rereads()).toHaveLength(0);
    view.unmount();
  }
});

test("further failures after a 404 never re-read the group again", async () => {
  vi.useFakeTimers();
  mount();
  await failPhoto();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(rereads()).toHaveLength(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(2000);
  });
  await failPhoto();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  await failPhoto();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(rereads()).toHaveLength(1);
  expect(calls.filter((call) => call.url.includes("/cover-art/"))).toHaveLength(
    1,
  );
});

test("leaving the page aborts a re-read in flight", async () => {
  const view = mount();
  await failPhoto();
  await vi.waitFor(() => expect(calls.length).toBeGreaterThan(0));
  view.unmount();
  expect(calls.every((call) => call.signal?.aborted)).toBe(true);
});

test("a group that already offers a credited photo never probes or re-reads", async () => {
  render(
    createElement(ArtistGroupPhoto, {
      account: "account",
      group: { ...group, credited_photo_artist_id: CREDIT },
    }),
  );
  await failPhoto();
  await act(async () => {});
  expect(calls).toHaveLength(0);
});
