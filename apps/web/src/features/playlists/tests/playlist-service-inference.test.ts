import type { Schema } from "../../../shared/schema";
import { expect, test } from "vitest";
import { inferPlaylistService } from "../model/playlistService";

const provider = (id: string, name: string): Schema<"PluginInstallation"> => ({
  id,
  service_id: `service-${id}`,
  revision: 1,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: `org.example.${id}`,
    name,
    protocol_version: 1,
    capabilities: ["playlists"],
  },
});
const spotify = provider("spotify", "Spotify");
const youtube = provider("youtube", "YouTube");
const jamendo = provider("jamendo", "Jamendo");
const all = [spotify, youtube, jamendo];

test.each([
  ["https://open.spotify.com/playlist/37i9dQ", "spotify"],
  ["open.spotify.com/playlist/37i9dQ", "spotify"],
  ["spotify:playlist:37i9dQ", "spotify"],
  ["https://music.youtube.com/playlist?list=PL1", "youtube"],
  ["https://www.youtube.com/playlist?list=PL1", "youtube"],
  ["https://youtu.be/abc", "youtube"],
  ["https://www.jamendo.com/playlist/500", "jamendo"],
])("%s is recognised as %s", (reference, id) => {
  const inferred = inferPlaylistService(reference, all, {});
  expect(inferred.provider?.id).toBe(id);
  expect(inferred.recognised).toBe(true);
  expect(inferred.ambiguous).toBe(false);
});

test("a service name given only in the account's service names also matches", () => {
  const renamed = {
    ...youtube,
    manifest: { ...youtube.manifest, name: "Video" },
  };
  const inferred = inferPlaylistService(
    "https://music.youtube.com/playlist?list=PL1",
    [spotify, renamed],
    { [renamed.service_id]: "YouTube Music" },
  );
  expect(inferred.provider?.id).toBe("youtube");
});

test("an unknown reference with several services is ambiguous", () => {
  const inferred = inferPlaylistService("PL12345", all, {});
  expect(inferred.provider).toBeNull();
  expect(inferred.ambiguous).toBe(true);
  expect(inferred.recognised).toBe(false);
});

test("a single service is used without being recognised from the link", () => {
  const inferred = inferPlaylistService(
    "https://provider.invalid/p/1",
    [spotify],
    {},
  );
  expect(inferred.provider?.id).toBe("spotify");
  expect(inferred.ambiguous).toBe(false);
  expect(inferred.recognised).toBe(false);
});

test("a link that names a service that is not connected stays ambiguous", () => {
  const inferred = inferPlaylistService(
    "https://open.spotify.com/playlist/1",
    [youtube, jamendo],
    {},
  );
  expect(inferred.provider).toBeNull();
  expect(inferred.ambiguous).toBe(true);
});

test.each([
  "https://open.spotify.com.evil.com/playlist/1",
  "https://spotify.evil.com/playlist/1",
  "https://notyoutube.com/playlist?list=1",
  "https://evil.com/open.spotify.com/playlist/1",
  "spotify.evil.com/playlist/1",
])("%s is not recognised", (reference) => {
  const inferred = inferPlaylistService(reference, all, {});
  expect(inferred.provider).toBeNull();
  expect(inferred.recognised).toBe(false);
});

test("play.spotify.com and www.jamendo.com are recognised hosts", () => {
  expect(
    inferPlaylistService("https://play.spotify.com/playlist/1", all, {})
      .provider?.id,
  ).toBe("spotify");
  expect(
    inferPlaylistService("https://www.jamendo.com/playlist/1", all, {}).provider
      ?.id,
  ).toBe("jamendo");
});
