import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import SettingsPage from "../pages/SettingsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";

const location = { url: "/settings" };
vi.mock("@inertiajs/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@inertiajs/react")>();
  return { ...actual, usePage: () => ({ url: location.url }) };
});
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ clear: vi.fn() }),
}));
vi.mock("../../library/components/LibrarySettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Music folders stub") };
});
vi.mock("../../metadata/components/MetadataPreferences", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Metadata stub") };
});
vi.mock("../../plugins/components/PluginSettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Plugins stub") };
});
vi.mock("../../community/components/CommunitySettings", async () => {
  const { createElement: h } = await import("react");
  const { useReportUnsaved } = await import("../../../shared/unsavedChanges");
  function CommunityStub() {
    useReportUnsaved(true);
    return h("p", null, "Community stub");
  }
  return { default: CommunityStub };
});
vi.mock("../../community/components/CommunityRankingSettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Ranking stub") };
});
vi.mock("../../session/components/AccountSecurity", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Account security stub") };
});
vi.mock("../../session/components/ScreenLockSettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Screen lock stub") };
});
vi.mock("../../accounts/components/AccountSettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Accounts stub") };
});
vi.mock("../../artists/components/SharedCorrectionsSettings", async () => {
  const { createElement: h } = await import("react");
  return { default: () => h("p", null, "Corrections stub") };
});

const admin: Schema<"SettingsPageProps"> = {
  user: { id: "alice", username: "alice", admin: true },
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  libraries: [],
  playlists: [],
};
const member: Schema<"SettingsPageProps"> = {
  ...admin,
  user: { id: "bob", username: "bob", admin: false },
};

function open(url: string, props: Schema<"SettingsPageProps"> = admin) {
  location.url = url;
  return render(
    createElement(NoticeProvider, null, createElement(SettingsPage, props)),
  );
}
const current = () =>
  within(screen.getByRole("navigation", { name: "Settings sections" }))
    .getAllByRole("link")
    .filter((link) => link.getAttribute("aria-current") === "page")
    .map((link) => link.textContent);

beforeEach(() => {
  location.url = "/settings";
  vi.spyOn(router, "visit").mockImplementation(() => undefined);
});

test("the navigation groups every section as a deep link", async () => {
  open("/settings?section=plugins");
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  expect(
    within(nav)
      .getAllByRole("link")
      .map((link) => [link.textContent, link.getAttribute("href")]),
  ).toEqual([
    ["Profile", "/settings?section=profile"],
    ["Privacy & lock", "/settings?section=privacy"],
    ["Music & providers", "/settings?section=music"],
    ["Plugins", "/settings?section=plugins"],
    ["Playback", "/settings?section=playback"],
    ["Downloads & storage", "/settings?section=downloads"],
    ["Accounts", "/settings?section=accounts"],
    ["Updates", "/settings?section=updates"],
    ["About", "/settings?section=about"],
  ]);
  for (const group of ["Your account", "Listening", "Server"])
    expect(within(nav).getByText(group)).toBeTruthy();
  expect(current()).toEqual(["Plugins"]);
  expect(await screen.findByText("Plugins stub")).toBeTruthy();
  expect(screen.queryByText("Music folders stub")).toBeNull();
});

test("members never see the Accounts section or reach it by URL", () => {
  open("/settings?section=accounts", member);
  const links = within(
    screen.getByRole("navigation", { name: "Settings sections" }),
  )
    .getAllByRole("link")
    .map((link) => link.textContent);
  expect(links).not.toContain("Accounts");
  expect(current()).toEqual(["Music & providers"]);
  expect(screen.queryByText("Accounts stub")).toBeNull();
});

test("legacy library and metadata links open Music & providers", async () => {
  for (const legacy of ["library", "metadata"]) {
    const view = open(`/settings?section=${legacy}`);
    expect(current()).toEqual(["Music & providers"]);
    expect(await screen.findByText("Music folders stub")).toBeTruthy();
    expect(screen.getByText("Metadata stub")).toBeTruthy();
    view.unmount();
  }
});

test("the privacy section holds consent, rankings and screen lock", async () => {
  open("/settings?section=privacy");
  expect(await screen.findByText("Community stub")).toBeTruthy();
  expect(screen.getByText("Ranking stub")).toBeTruthy();
  expect(screen.getByText("Screen lock stub")).toBeTruthy();
});

test("administrators reach accounts and shared corrections", async () => {
  open("/settings?section=accounts");
  expect(await screen.findByText("Accounts stub")).toBeTruthy();
  expect(screen.getByText("Corrections stub")).toBeTruthy();
});

test("without a section the page shows the list view on phones and Music by default", () => {
  const view = open("/settings");
  expect(
    view.container.querySelector(".settings")?.getAttribute("data-view"),
  ).toBe("list");
  expect(current()).toEqual(["Music & providers"]);
  view.unmount();
  const chosen = open("/settings?section=about");
  expect(
    chosen.container.querySelector(".settings")?.getAttribute("data-view"),
  ).toBe("section");
  expect(
    screen.getByRole("link", { name: "All settings" }).getAttribute("href"),
  ).toBe("/settings");
});

test("the profile shows the signed-in account and keeps sign-out and lock reachable", () => {
  const view = open("/settings?section=profile");
  expect(screen.getByRole("heading", { name: "Profile" })).toBeTruthy();
  expect(screen.getByText("alice")).toBeTruthy();
  expect(screen.getByText("Administrator")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Sign out" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Lock now" })).toBeNull();
  view.unmount();
  open("/settings?section=profile", {
    ...admin,
    lock: { pin_enabled: true, idle_minutes: 5, locked: false },
  });
  expect(screen.getByRole("button", { name: "Lock now" })).toBeTruthy();
});

test("the profile header shows the display name with the username beneath, or the username alone", () => {
  const named = open("/settings?section=profile", {
    ...admin,
    user: { ...admin.user, display_name: "Alice <b>Admin</b>" },
  });
  const identity = named.container.querySelector(".profile-identity")!;
  expect(identity.querySelector(".setting-line-title")?.textContent).toBe(
    "Alice <b>Admin</b>",
  );
  expect(identity.querySelector(".setting-line-hint")?.textContent).toBe(
    "alice · Administrator",
  );
  expect(identity.querySelector(".avatar")?.textContent).toBe("AB");
  expect(identity.querySelector("b")).toBeNull();
  expect(
    (screen.getByRole("textbox", { name: "Display name" }) as HTMLInputElement)
      .value,
  ).toBe("Alice <b>Admin</b>");
  named.unmount();
  const plain = open("/settings?section=profile");
  const bare = plain.container.querySelector(".profile-identity")!;
  expect(bare.querySelector(".setting-line-title")?.textContent).toBe("alice");
  expect(bare.querySelector(".setting-line-hint")?.textContent).toBe(
    "Administrator",
  );
  expect(bare.querySelector(".avatar")?.textContent).toBe("A");
});

test("opening a section from the list focuses its heading and going back focuses the page heading", () => {
  const view = open("/settings");
  const rerender = (url: string) => {
    location.url = url;
    view.rerender(
      createElement(NoticeProvider, null, createElement(SettingsPage, admin)),
    );
  };
  rerender("/settings?section=music");
  expect(document.activeElement).toBe(
    screen.getByRole("heading", { name: "Music & providers", level: 2 }),
  );
  rerender("/settings");
  expect(document.activeElement).toBe(
    screen.getByRole("heading", { name: "Settings", level: 1 }),
  );
});

test("a member opening the accounts link has the address replaced with the resolved section", () => {
  window.history.replaceState(null, "", "/settings?section=accounts");
  open("/settings?section=accounts", member);
  expect(window.location.search).toBe("?section=music");
  window.history.replaceState(null, "", "/settings?section=library");
  open("/settings?section=library");
  expect(window.location.search).toBe("?section=music");
});

test("leaving a section with unsaved changes asks first and stays when declined", async () => {
  open("/settings?section=privacy");
  await screen.findByText("Community stub");
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  const plugins = screen.getByRole("link", { name: "Plugins" });
  expect(fireEvent.click(plugins)).toBe(false);
  expect(confirm).toHaveBeenCalledTimes(1);
  confirm.mockReturnValue(true);
  fireEvent.click(plugins);
  expect(confirm).toHaveBeenCalledTimes(2);
});

test("sections without unsaved changes never prompt", () => {
  open("/settings?section=about");
  const confirm = vi.spyOn(window, "confirm");
  fireEvent.click(screen.getByRole("link", { name: "Plugins" }));
  expect(confirm).not.toHaveBeenCalled();
});

const searchBox = () =>
  screen.getByRole("combobox", { name: "Search settings" });
const typeQuery = (text: string) =>
  fireEvent.change(searchBox(), { target: { value: text } });

test("the navigation starts with a search field and no results", () => {
  open("/settings?section=music");
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  expect(
    within(nav).getByRole("combobox", { name: "Search settings" }),
  ).toBeTruthy();
  expect(searchBox().getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("typing lists matching sections and settings and hides the section links", () => {
  open("/settings?section=music");
  typeQuery("password");
  expect(searchBox().getAttribute("aria-expanded")).toBe("true");
  const options = within(screen.getByRole("listbox")).getAllByRole("option");
  expect(options[0].textContent).toContain("Change password");
  expect(options[0].textContent).toContain("Profile");
  expect(
    within(
      screen.getByRole("navigation", { name: "Settings sections" }),
    ).queryByRole("link", { name: "Playback" }),
  ).toBeNull();
});

test("choosing a result opens its section at the setting", () => {
  open("/settings?section=music");
  typeQuery("password");
  fireEvent.click(screen.getAllByRole("option")[0]);
  expect(router.visit).toHaveBeenCalledWith(
    "/settings?section=profile#setting-change-password",
    expect.objectContaining({ preserveState: true }),
  );
  expect((searchBox() as HTMLInputElement).value).toBe("");
});

test("arrow keys move through results and Enter chooses the active one", () => {
  open("/settings?section=music");
  typeQuery("plugin");
  const options = screen.getAllByRole("option");
  expect(searchBox().getAttribute("aria-activedescendant")).toBe(options[0].id);
  fireEvent.keyDown(searchBox(), { key: "ArrowDown" });
  expect(searchBox().getAttribute("aria-activedescendant")).toBe(options[1].id);
  expect(options[1].getAttribute("aria-selected")).toBe("true");
  fireEvent.keyDown(searchBox(), { key: "Enter" });
  expect(router.visit).toHaveBeenCalledTimes(1);
});

test("Escape clears the query and brings the section links back", () => {
  open("/settings?section=music");
  typeQuery("plugin");
  fireEvent.keyDown(searchBox(), { key: "Escape" });
  expect((searchBox() as HTMLInputElement).value).toBe("");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(screen.getByRole("link", { name: "Playback" })).toBeTruthy();
});

test("a query with no match says so", () => {
  open("/settings?section=music");
  typeQuery("zebra");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("No settings match");
});

test("slash focuses the search from the page", () => {
  open("/settings?section=music");
  fireEvent.keyDown(document.body, { key: "/" });
  expect(document.activeElement).toBe(searchBox());
});

test("a setting link scrolls to the card, focuses it and highlights it briefly", async () => {
  const scroll = vi.fn();
  Element.prototype.scrollIntoView = scroll;
  open("/settings?section=profile#setting-account");
  const card = await vi.waitFor(() => {
    const found = document.getElementById("setting-account");
    if (!found?.hasAttribute("data-highlight")) throw new Error("not yet");
    return found;
  });
  expect(scroll).toHaveBeenCalled();
  expect(document.activeElement).toBe(
    within(card).getByRole("heading", { name: "Account" }),
  );
});
