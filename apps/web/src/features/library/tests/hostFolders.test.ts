import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  },
}));
const props: Schema<"SettingsPageProps"> = {
  errors: {},
  native_playback: true,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "admin", username: "Admin", admin: true },
  libraries: [{ id: "library", name: "Home library" }],
  playlists: [],
};
const music: Schema<"DirectoryRef"> = {
  directory_id: "legacy-music",
  components: [],
};
const connected: Schema<"MusicFolder"> = {
  id: "folder",
  library_id: "library",
  host_device: "host",
  name: "Music folder",
  revision: 1,
  connected: true,
  available: true,
  last_scan_at: null,
  directory: music,
  latest_job: null,
};

function show() {
  return render(
    createElement(NoticeProvider, null, createElement(LibrarySettings, props)),
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(storageApi.folders).mockResolvedValue([]);
  vi.mocked(storageApi.directories).mockResolvedValue([]);
  vi.mocked(storageApi.connect).mockResolvedValue(connected);
});

test("a desktop proposes the Music folder and adds nothing until confirmed", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: music,
  });
  show();
  await screen.findByRole("heading", { name: "Add your Music folder?" });
  expect(storageApi.connect).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Use Music folder" }));
  await waitFor(() =>
    expect(storageApi.connect).toHaveBeenCalledWith(
      "admin",
      "library",
      music,
      expect.any(AbortSignal),
    ),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("heading", { name: "Add your Music folder?" }),
    ).toBeNull(),
  );
});

test("declining the proposal adds nothing", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: music,
  });
  show();
  fireEvent.click(await screen.findByRole("button", { name: "Not now" }));
  expect(
    screen.queryByRole("heading", { name: "Add your Music folder?" }),
  ).toBeNull();
  expect(storageApi.connect).not.toHaveBeenCalled();
});

test("an already connected Music folder is not proposed again", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: { directory_id: "legacy-music", components: ["Jazz"] },
  });
  vi.mocked(storageApi.folders).mockResolvedValue([connected]);
  show();
  await screen.findByRole("heading", { name: "Music folder" });
  await screen.findByRole("button", { name: "Choose folder…" });
  expect(
    screen.queryByRole("heading", { name: "Add your Music folder?" }),
  ).toBeNull();
});

test("a missing Music folder gives no proposal but keeps the picker", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  show();
  await screen.findByRole("button", { name: "Choose folder…" });
  expect(
    screen.queryByRole("heading", { name: "Add your Music folder?" }),
  ).toBeNull();
});

test("the native picker result is connected through the existing folder route", async () => {
  const chosen: Schema<"DirectoryRef"> = {
    directory_id: "host-1",
    components: [],
  };
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  vi.mocked(storageApi.chooseHostFolder).mockResolvedValue({
    directory: chosen,
  });
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose folder…" }),
  );
  await waitFor(() =>
    expect(storageApi.connect).toHaveBeenCalledWith(
      "admin",
      "library",
      chosen,
      expect.any(AbortSignal),
    ),
  );
});

test("cancelling the native picker connects nothing", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  vi.mocked(storageApi.chooseHostFolder).mockResolvedValue({ directory: null });
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose folder…" }),
  );
  await waitFor(() => expect(storageApi.chooseHostFolder).toHaveBeenCalled());
  await screen.findByRole("button", { name: "Choose folder…" });
  expect(storageApi.connect).not.toHaveBeenCalled();
});

test("a failed native selection is reported", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  vi.mocked(storageApi.chooseHostFolder).mockRejectedValue(
    new Error("Choose a folder that exists and can be read"),
  );
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose folder…" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "can be read",
  );
});

test("browsers show neither a picker nor a proposal", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: false,
    suggestion: null,
  });
  show();
  await screen.findByRole("button", { name: "Add music folder" });
  expect(screen.queryByRole("button", { name: "Choose folder…" })).toBeNull();
  expect(
    screen.queryByRole("heading", { name: "Add your Music folder?" }),
  ).toBeNull();
  expect(screen.getByText(/Share it in Hito Setup on the server/)).toBeTruthy();
});

test("a failed offer lookup leaves the browser behaviour", async () => {
  vi.mocked(storageApi.hostOffer).mockRejectedValue(new Error("offline"));
  show();
  await screen.findByRole("button", { name: "Add music folder" });
  expect(screen.queryByRole("button", { name: "Choose folder…" })).toBeNull();
});

test("a suggestion that contains a connected folder is not proposed", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: music,
  });
  vi.mocked(storageApi.folders).mockResolvedValue([
    {
      ...connected,
      directory: { directory_id: "legacy-music", components: ["Jazz"] },
    },
  ]);
  show();
  await screen.findByRole("button", { name: "Choose folder…" });
  expect(
    screen.queryByRole("heading", { name: "Add your Music folder?" }),
  ).toBeNull();
});

test("the proposal cannot be accepted while the chooser is open", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: music,
  });
  vi.mocked(storageApi.chooseHostFolder).mockReturnValue(
    new Promise(() => undefined),
  );
  show();
  await screen.findByRole("heading", { name: "Add your Music folder?" });
  fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
  await screen.findByRole("button", { name: "Waiting for your choice…" });
  expect(
    screen
      .getByRole("button", { name: "Use Music folder" })
      .hasAttribute("disabled"),
  ).toBe(true);
});

test("a choice finishing after the page is closed connects nothing", async () => {
  let finish: (choice: {
    directory: Schema<"DirectoryRef"> | null;
  }) => void = () => {};
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  vi.mocked(storageApi.chooseHostFolder).mockImplementation(
    (_account, signal) => {
      expect(signal.aborted).toBe(false);
      return new Promise((resolve) => {
        finish = resolve;
      });
    },
  );
  const view = show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose folder…" }),
  );
  await waitFor(() => expect(storageApi.chooseHostFolder).toHaveBeenCalled());
  const signal = vi.mocked(storageApi.chooseHostFolder).mock.calls[0][1];
  view.unmount();
  expect(signal.aborted).toBe(true);
  finish({ directory: { directory_id: "host-1", components: [] } });
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(storageApi.connect).not.toHaveBeenCalled();
});

test("a busy chooser is explained", async () => {
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: true,
    suggestion: null,
  });
  vi.mocked(storageApi.chooseHostFolder).mockRejectedValue(
    new Error("A folder chooser is already open"),
  );
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose folder…" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "already open",
  );
});
