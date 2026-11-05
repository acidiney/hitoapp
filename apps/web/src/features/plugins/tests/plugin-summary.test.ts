import type { Schema } from "../../../shared/schema";
import { expect, test } from "vitest";
import {
  capabilityChips,
  newCapabilities,
  serviceSummary,
} from "../model/pluginSummary";

type Capability = Schema<"Manifest">["capabilities"][number];
const caps = (...values: Capability[]) => values;

test("a search and playback service reads as one plain sentence", () => {
  expect(serviceSummary("YouTube", caps("search", "metadata", "sources"))).toBe(
    "Finds songs and plays them from YouTube, and fills in track details.",
  );
});

test("a playlist and artwork service names what it does for the listener", () => {
  expect(serviceSummary("Spotify", caps("playlists", "artwork"))).toBe(
    "Imports and follows Spotify playlists, and fetches album covers.",
  );
});

test("a service named after its playlists does not repeat the word", () => {
  expect(serviceSummary("Spotify public playlists", caps("playlists"))).toBe(
    "Imports and follows Spotify public playlists.",
  );
  expect(serviceSummary("Radio Playlist", caps("playlists"))).toBe(
    "Imports and follows Radio Playlist.",
  );
});

test("single abilities keep the service name where it helps", () => {
  expect(serviceSummary("Jamendo", caps("sources"))).toBe(
    "Plays songs from Jamendo.",
  );
  expect(serviceSummary("Jamendo", caps("search"))).toBe(
    "Finds songs on Jamendo.",
  );
  expect(serviceSummary("Genius", caps("lyrics"))).toBe("Shows lyrics.");
});

test("a service that declares nothing says so", () => {
  expect(serviceSummary("Empty", caps())).toBe(
    "Does not offer anything to listen to yet.",
  );
});

test("new capabilities are named in listener words", () => {
  expect(
    newCapabilities(
      caps("search", "sources"),
      caps("search", "sources", "artwork"),
    ),
  ).toEqual(["album covers"]);
  expect(
    newCapabilities(caps("search"), caps("search", "lyrics", "playlists")),
  ).toEqual(["playlist import", "lyrics"]);
  expect(newCapabilities(caps("search"), caps("search"))).toEqual([]);
});

test("featured music is named in the summary and in the changes to accept", () => {
  expect(serviceSummary("Jamendo", caps("playlists", "featured"))).toBe(
    "Imports and follows Jamendo playlists, and features music on Discover.",
  );
  expect(
    newCapabilities(
      caps("playlists"),
      caps("playlists", "featured", "artwork"),
    ),
  ).toEqual(["album covers", "featured music on Discover"]);
});

test("capability chips use plain labels in a fixed order", () => {
  expect(capabilityChips(caps("sources", "search", "metadata"))).toEqual([
    "Search",
    "Playback",
    "Track details",
  ]);
  expect(
    capabilityChips(
      caps(
        "dashboard",
        "featured",
        "backing_tracks",
        "source_preparation",
        "artwork",
        "lyrics",
        "catalog",
        "playlists",
      ),
    ),
  ).toEqual([
    "Playlists",
    "Browse",
    "Lyrics",
    "Covers",
    "Faster playback",
    "Backing tracks",
    "Featured",
    "Dashboard",
  ]);
  expect(capabilityChips(caps())).toEqual([]);
  expect(capabilityChips(caps("search", "search"))).toEqual(["Search"]);
});
