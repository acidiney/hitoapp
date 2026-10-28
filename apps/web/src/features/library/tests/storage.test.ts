import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import LibrarySettings from "../components/LibrarySettings";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { storageApi } from "../api/storageApi";
import type { MusicFolder, SettingsPageProps } from "../../../shared/types";

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
  },
}));
const props: SettingsPageProps = {
  errors: {},
  native_playback: false,
  user: { id: "admin", username: "Admin", admin: true },
  libraries: [{ id: "library", name: "Home library" }],
  playlists: [],
};
const folder: MusicFolder = {
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
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  vi.mocked(storageApi.folders).mockResolvedValue([]);
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: false,
    suggestion: null,
  });
  vi.mocked(storageApi.directories).mockResolvedValue([
    { id: "music", name: "My music", available: true },
  ]);
  vi.mocked(storageApi.browse).mockImplementation(
    async (_account, directory) => ({
      directory,
      next_offset: null,
      directories: directory.components?.length
        ? []
        : [
            {
              name: "Albums",
              directory: { directory_id: "music", components: ["Albums"] },
            },
          ],
    }),
  );
});
test("an administrator selects a shared directory through the folder browser", async () => {
  vi.mocked(storageApi.connect).mockResolvedValue(folder);
  vi.mocked(storageApi.folders)
    .mockResolvedValueOnce([])
    .mockResolvedValue([folder]);
  render(
    createElement(NoticeProvider, null, createElement(LibrarySettings, props)),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Add music folder" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Open My music" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open Albums" }));
  await screen.findByText("This folder has no subfolders.");
  fireEvent.click(screen.getByRole("button", { name: "Use this folder" }));
  await waitFor(() =>
    expect(storageApi.connect).toHaveBeenCalledWith(
      "admin",
      "library",
      { directory_id: "music", components: ["Albums"] },
      expect.any(AbortSignal),
    ),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(await screen.findByRole("heading", { name: "Albums" })).toBeTruthy();
  expect(screen.queryByText(/HITO_MUSIC_FOLDER/)).toBeNull();
});

test("ordinary listeners do not request folder administration", async () => {
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(LibrarySettings, {
        ...props,
        user: { ...props.user, admin: false },
      }),
    ),
  );
  expect(screen.getByText("Home library")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Add music folder" })).toBeNull();
  expect(storageApi.folders).not.toHaveBeenCalled();
  expect(storageApi.directories).not.toHaveBeenCalled();
});

test("personal provider libraries are excluded from folder administration", async () => {
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(LibrarySettings, {
        ...props,
        libraries: [
          { id: "private", name: "Personal providers", personal: true },
          ...props.libraries,
        ],
      }),
    ),
  );
  await screen.findByRole("button", { name: "Add music folder" });
  expect(storageApi.folders).toHaveBeenCalledWith(
    "admin",
    "library",
    expect.any(AbortSignal),
  );
  expect(screen.queryByRole("combobox", { name: "Library" })).toBeNull();
  expect(
    screen.getByText("Your personal provider library is managed in Plugins."),
  ).toBeTruthy();
});

test("failed folder connection preserves the selection and shows a retryable error", async () => {
  vi.mocked(storageApi.connect).mockRejectedValueOnce(
    new Error("Folder unavailable. Try again."),
  );
  render(
    createElement(NoticeProvider, null, createElement(LibrarySettings, props)),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Add music folder" }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Open My music" }));
  fireEvent.click(await screen.findByRole("button", { name: "Open Albums" }));
  await screen.findByText("This folder has no subfolders.");
  fireEvent.click(screen.getByRole("button", { name: "Use this folder" }));
  await screen.findByRole("alert");
  await waitFor(() => expect(storageApi.folders).toHaveBeenCalledTimes(3));
  expect(screen.getByRole("dialog").textContent).toContain(
    "Folder unavailable. Try again.",
  );
  expect(
    screen
      .getByRole("button", { name: "Use this folder" })
      .hasAttribute("disabled"),
  ).toBe(false);
  expect(
    screen.getByRole("navigation", { name: "Folder location" }).textContent,
  ).toContain("Albums");
});
