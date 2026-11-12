import { expect, test } from "vitest";
import { importProblem } from "../model/importProblem";
import {
  looksLikePlaylistLink,
  playlistLinkService,
} from "../model/playlistService";

test("a missing answer never claims the playlist was or was not saved", () => {
  const problem = importProblem(undefined, "Spotify", "raw", true);
  expect(problem.title).toBe("Hito didn't get an answer");
  expect(problem.text).toContain("does not import it twice");
  expect(problem.text).not.toContain("Nothing was saved");
});

test("refusals send the listener to Plugins and say nothing else changed", () => {
  for (const status of [401, 403])
    expect(importProblem(status, "Spotify", "raw", true)).toMatchObject({
      title: "Spotify refused",
      actions: ["plugins"],
    });
});

test("a playlist the service does not return can be edited or retried", () => {
  expect(importProblem(404, "Spotify", "raw", false)).toMatchObject({
    title: "Spotify didn't return this playlist",
    actions: ["edit"],
  });
});

test("the follow limit offers a copy instead and the import limit does not", () => {
  expect(importProblem(429, "Spotify", "raw", true).actions).toEqual(["copy"]);
  expect(importProblem(429, "Spotify", "raw", false).actions).toEqual([]);
});

test("a busy plugin says nothing was saved", () => {
  for (const status of [502, 503, 504])
    expect(importProblem(status, "Spotify", "raw", true).title).toBe(
      "The Spotify plugin is busy",
    );
});

test("any other failure shows the server's own sentence", () => {
  expect(importProblem(500, "Spotify", "The server said no.", true)).toEqual({
    title: "Hito couldn't import this playlist",
    text: "The server said no.",
    actions: ["edit"],
  });
});

test("only playlist links from known services open the import dialog", () => {
  expect(
    looksLikePlaylistLink("https://open.spotify.com/playlist/37i9dQ"),
  ).toBe(true);
  expect(
    looksLikePlaylistLink("https://music.youtube.com/playlist?list=PL1"),
  ).toBe(true);
  expect(looksLikePlaylistLink("spotify:playlist:37i9dQ")).toBe(true);
  expect(looksLikePlaylistLink("https://open.spotify.com/album/1")).toBe(false);
  expect(looksLikePlaylistLink("stromae")).toBe(false);
  expect(looksLikePlaylistLink("a playlist of mine")).toBe(false);
  expect(playlistLinkService("https://open.spotify.com/playlist/1")).toBe(
    "Spotify",
  );
});
