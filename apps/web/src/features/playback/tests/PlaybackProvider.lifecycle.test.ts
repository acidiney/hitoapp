import { createElement, StrictMode } from "react";
import { render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../components/PlaybackProvider";

test("Strict Mode effect replay retains one owned browser audio element and final unmount removes it", () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const fetch = vi.fn(async () => {
    throw new Error("Mounting a player must not request audio or downloads");
  });
  vi.stubGlobal("fetch", fetch);
  const previous = new Set(document.querySelectorAll("audio"));
  const { unmount } = render(
    createElement(
      StrictMode,
      null,
      createElement(
        NoticeProvider,
        null,
        createElement(PlaybackProvider, {
          account: "owned-strict-player-account",
          children: createElement("span", null, "Owned browser player"),
        }),
      ),
    ),
  );

  try {
    expect(document.querySelectorAll("audio")).toHaveLength(1);
    expect(document.querySelectorAll("audio[data-hito-player]")).toHaveLength(
      1,
    );
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(play).not.toHaveBeenCalled();
    expect(fetch).not.toHaveBeenCalled();
    unmount();
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(fetch).not.toHaveBeenCalled();
  } finally {
    unmount();
    for (const audio of document.querySelectorAll("audio"))
      if (!previous.has(audio)) audio.remove();
  }
});
