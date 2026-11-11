import { expect, test } from "vitest";
import { searchPlacement } from "../searchPlacement";

test.each(["HomePage", "GenrePage", "QueuePage"])(
  "%s gets the global search in the top bar on a desktop",
  (page) => {
    expect(searchPlacement(page, false)).toBe("global");
  },
);

test.each(["HomePage", "GenrePage", "QueuePage"])(
  "%s has no top bar search on a phone, the Search tab leads there",
  (page) => {
    expect(searchPlacement(page, true)).toBe("none");
  },
);

test("Settings owns its search field, except the Plugins page which keeps the top bar", () => {
  expect(searchPlacement("SettingsPage", false)).toBe("none");
  expect(searchPlacement("SettingsPage", false, "privacy")).toBe("none");
  expect(searchPlacement("SettingsPage", true)).toBe("none");
  expect(searchPlacement("SettingsPage", false, "plugins")).toBe("global");
  expect(searchPlacement("SettingsPage", true, "plugins")).toBe("none");
});

test.each(["SearchPage", "DiscoverPage"])(
  "%s owns its field on every screen, so the top bar adds none",
  (page) => {
    expect(searchPlacement(page, false)).toBe("none");
    expect(searchPlacement(page, true)).toBe("none");
  },
);

test.each([
  "LibraryPage",
  "AlbumIndexPage",
  "ArtistIndexPage",
  "PlaylistIndexPage",
])(
  "%s keeps the library filter in the top bar on every screen size",
  (page) => {
    expect(searchPlacement(page, false)).toBe("library");
    expect(searchPlacement(page, true)).toBe("library");
  },
);

test("the track details page has no search field", () => {
  expect(searchPlacement("RecordingDetailsPage", false)).toBe("none");
  expect(searchPlacement("RecordingDetailsPage", true)).toBe("none");
});
