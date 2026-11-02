import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ArtistPhoto from "../components/ArtistPhoto";

const CREDIT = "0123456789abcdef0123456789abcdef";

afterEach(cleanup);

test("a library credit with a provider photo asks the credited artist route", () => {
  render(
    createElement(ArtistPhoto, {
      kind: "credited-artist",
      artist: CREDIT,
      identity: CREDIT,
      name: "Nova",
    }),
  );
  expect(document.querySelector("img")?.getAttribute("src")).toBe(
    `/api/v1/cover-art/credited-artist/${CREDIT}`,
  );
});

test("a library credit without a provider photo never requests an image", () => {
  render(
    createElement(
      ArtistPhoto,
      {
        kind: "credited-artist",
        artist: CREDIT,
        identity: CREDIT,
        name: "Nova",
        enabled: false,
      },
      createElement("p", null, "Album cover instead"),
    ),
  );
  expect(document.querySelector("img")).toBeNull();
  expect(screen.getByText("Album cover instead")).toBeTruthy();
});

test("a failed credited photo waits two seconds and then asks again", async () => {
  vi.useFakeTimers();
  try {
    render(
      createElement(
        ArtistPhoto,
        {
          kind: "credited-artist",
          artist: CREDIT,
          identity: CREDIT,
          name: "Nova",
        },
        createElement("p", null, "Album cover instead"),
      ),
    );
    const source = `/api/v1/cover-art/credited-artist/${CREDIT}`;
    fireEvent.error(document.querySelector("img")!);
    expect(document.querySelector("img")).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1999);
    });
    expect(document.querySelector("img")).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(document.querySelector("img")?.getAttribute("src")).toBe(source);
    fireEvent.error(document.querySelector("img")!);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4999);
    });
    expect(document.querySelector("img")).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(document.querySelector("img")?.getAttribute("src")).toBe(source);
    fireEvent.error(document.querySelector("img")!);
    expect(screen.getByText("Album cover instead")).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});

test("music brainz photos still require a music brainz identifier", () => {
  render(
    createElement(
      ArtistPhoto,
      { artist: CREDIT, identity: CREDIT, name: "Nova" },
      createElement("p", null, "Album cover instead"),
    ),
  );
  expect(document.querySelector("img")).toBeNull();
});
