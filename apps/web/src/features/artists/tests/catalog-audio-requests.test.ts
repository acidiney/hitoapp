import { renderHook, act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useCatalogPlayback } from "../hooks/useCatalogPlayback";
import { artistEditionApi } from "../api/artistEditionApi";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    prepareSelection: () => async () => false,
    snapshot: () => ({ shuffle: false }),
    shuffle: vi.fn(),
    queueSnapshot: () => ({ currentIndex: null, total: 0 }),
    subscribe: () => () => {},
  }),
}));

const context = {
  account: "account",
  recording: "recording",
  association: { id: "association" },
  providerArtistId: "artist",
  releaseGroupId: "group",
} as unknown as ArtistEditionContext;
const occurrences = ["one", "two", "three"];
const remembered = new Map<string, never>();

test("an unmatched track is asked once until the user retries it", async () => {
  const audio = vi
    .spyOn(artistEditionApi, "audio")
    .mockResolvedValue({ status: "no_match" } as never);
  const { result } = renderHook(() =>
    useCatalogPlayback(context, "edition", occurrences, remembered),
  );
  await act(async () => {
    await result.current.start(0);
  });
  expect(audio).toHaveBeenCalledTimes(3);
  await act(async () => {
    await result.current.start(0);
  });
  expect(audio).toHaveBeenCalledTimes(4);
  await act(async () => {
    await result.current.start(0);
  });
  expect(audio).toHaveBeenCalledTimes(5);
  await act(async () => {
    await result.current.find(2);
  });
  expect(audio).toHaveBeenCalledTimes(6);
  audio.mockRestore();
});
