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

function renderSettings() {
  render(
    createElement(NoticeProvider, null, createElement(LibrarySettings, props)),
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  vi.mocked(storageApi.folders).mockResolvedValue([]);
  vi.mocked(storageApi.hostOffer).mockResolvedValue({
    picker: false,
    suggestion: null,
  });
});

test("removing missing tracks asks first and reports what was removed and what stays", async () => {
  vi.mocked(storageApi.removeMissingRecordings).mockResolvedValue({
    removed: 3,
    kept_referenced: 2,
    more: false,
  });
  renderSettings();
  const opener = await screen.findByRole("button", {
    name: "Remove missing tracks now",
  });
  opener.focus();
  fireEvent.click(opener);
  expect(storageApi.removeMissingRecordings).not.toHaveBeenCalled();
  const dialog = screen.getByRole("dialog", {
    name: "Remove missing tracks now?",
  });
  expect(within(dialog).getByText(/Your files are not touched/)).toBeTruthy();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Keep them for now" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(opener);
  fireEvent.click(opener);
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Remove missing tracks",
    }),
  );
  await waitFor(() =>
    expect(storageApi.removeMissingRecordings).toHaveBeenCalledWith(
      "admin",
      "library",
      expect.any(AbortSignal),
    ),
  );
  const outcome = await screen.findByText(/Removed 3 tracks/);
  expect(outcome.textContent).toContain("2 stay because");
  expect(outcome.closest("[role=status]")).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("a large cleanup says when more tracks remain and a failure can be retried", async () => {
  vi.mocked(storageApi.removeMissingRecordings)
    .mockRejectedValueOnce(new Error("Network down"))
    .mockResolvedValueOnce({ removed: 2000, kept_referenced: 0, more: true });
  renderSettings();
  const open = async () => {
    fireEvent.click(
      await screen.findByRole("button", { name: "Remove missing tracks now" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Remove missing tracks",
      }),
    );
  };
  await open();
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Network down",
  );
  await open();
  expect(
    (await screen.findByText(/Removed 2,000 tracks/)).textContent,
  ).toContain("Remove again to continue");
  expect(screen.queryByRole("alert")).toBeNull();
});

test("nothing to remove is reported plainly", async () => {
  vi.mocked(storageApi.removeMissingRecordings).mockResolvedValue({
    removed: 0,
    kept_referenced: 0,
    more: false,
  });
  renderSettings();
  fireEvent.click(
    await screen.findByRole("button", { name: "Remove missing tracks now" }),
  );
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "Remove missing tracks",
    }),
  );
  expect(await screen.findByText("No missing tracks to remove.")).toBeTruthy();
});
