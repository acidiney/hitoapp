import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import LibrarySettings from "../components/LibrarySettings";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { storageApi } from "../api/storageApi";

vi.mock("../api/storageApi", () => ({
  storageApi: {
    directories: vi.fn(),
    browse: vi.fn(),
    hostOffer: vi.fn(),
    chooseHostFolder: vi.fn(),
    folders: vi.fn(),
    connect: vi.fn(),
    remove: vi.fn(),
    scan: vi.fn(),
    removeMissingRecordings: vi.fn(),
  },
}));
const props: Schema<"SettingsPageProps"> = {
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "admin", username: "Admin", admin: true },
  libraries: [{ id: "library", name: "Home library" }],
  playlists: [],
};
const folder: Schema<"MusicFolder"> = {
  id: "folder",
  library_id: "library",
  host_device: "host",
  name: "Albums",
  revision: 1,
  connected: true,
  available: true,
  last_scan_at: null,
  directory: { directory_id: "music", components: ["Albums"] },
  latest_job: null,
};

beforeEach(() => {
  vi.resetAllMocks();
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  vi.mocked(storageApi.folders).mockResolvedValue([folder]);
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: false,
    suggestion: null,
  });
  render(
    createElement(NoticeProvider, null, createElement(LibrarySettings, props)),
  );
});

test("each folder scans on its own and one button scans them all", async () => {
  await screen.findByRole("button", { name: "Scan Albums" });
  expect(screen.getByText("Scan folder")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Scan all folders" })).toBeTruthy();
});

test("removing a folder is confirmed in a dialog that returns focus when kept", async () => {
  const opener = await screen.findByRole("button", { name: "Remove Albums" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = screen.getByRole("dialog", { name: "Remove Albums?" });
  expect(storageApi.remove).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Keep folder" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(opener);
  expect(storageApi.remove).not.toHaveBeenCalled();
});

test("confirming the removal removes the folder and closes the dialog", async () => {
  vi.mocked(storageApi.remove).mockResolvedValue();
  fireEvent.click(await screen.findByRole("button", { name: "Remove Albums" }));
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Remove folder",
    }),
  );
  await waitFor(() =>
    expect(storageApi.remove).toHaveBeenCalledWith(
      "admin",
      folder,
      expect.any(AbortSignal),
    ),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("a failed removal keeps the dialog open and says why", async () => {
  vi.mocked(storageApi.remove).mockRejectedValue(new Error("Network down"));
  fireEvent.click(await screen.findByRole("button", { name: "Remove Albums" }));
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Remove folder",
    }),
  );
  const alert = await within(screen.getByRole("dialog")).findByRole("alert");
  expect(alert.textContent).toContain("Network down");
});
