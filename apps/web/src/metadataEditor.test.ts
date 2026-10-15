// @vitest-environment jsdom
import { createElement } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { change } from "./api";
import MetadataEditor from "./components/MetadataEditor";
import { NoticeProvider } from "./components/Notices";

vi.mock("./api", () => ({ change: vi.fn() }));
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => ({ setRecordingField: vi.fn() }),
}));
vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

test("a pending field save protects the other draft from a concurrent reload", async () => {
  let resolve!: (value: { acknowledged: string[] }) => void;
  vi.mocked(change).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const inherited = {
    value: "Source",
    inherited: "Source",
    personal: null,
    operation_id: null,
  };
  render(
    createElement(MetadataEditor, {
      account: "alice",
      target: { kind: "recording", id: "track" },
      metadata: { title: inherited, artist: inherited },
    }),
    { wrapper: NoticeProvider },
  );
  const title = screen.getByLabelText("Track title") as HTMLInputElement;
  const artist = screen.getByLabelText("Track artist") as HTMLInputElement;
  fireEvent.change(artist, { target: { value: "Unsaved artist" } });
  fireEvent.change(title, { target: { value: "Personal title" } });
  fireEvent.click(screen.getByRole("button", { name: "Save personal title" }));
  expect(artist.disabled).toBe(true);
  expect(screen.getAllByRole("button", { name: "Saving…" })).toHaveLength(2);
  resolve({ acknowledged: [] });
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(artist.value).toBe("Unsaved artist");
  expect(change).toHaveBeenCalledTimes(1);
});
