import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import FolderRow from "../components/FolderRow";

function folderWith(
  failure: Schema<"Job">["failure"],
  error: string,
): Schema<"MusicFolder"> {
  return {
    id: "folder",
    library_id: "library",
    host_device: "host",
    name: "Albums",
    revision: 1,
    connected: true,
    available: true,
    last_scan_at: null,
    directory: { directory_id: "music", components: ["Albums"] },
    latest_job: {
      id: "job",
      library_id: "library",
      folder_id: "folder",
      kind: "scan",
      state: "failed",
      progress: 0,
      error,
      failure,
    },
  };
}

function show(folder: Schema<"MusicFolder">) {
  render(
    createElement(FolderRow, {
      folder,
      busy: false,
      pending: false,
      removing: false,
      onScan: vi.fn(),
      onAskRemove: vi.fn(),
      onCancelRemove: vi.fn(),
      onRemove: vi.fn(),
    }),
  );
}

test("missing FFmpeg tools are named with the install step", () => {
  show(folderWith("media_tools_missing", "FFmpeg is not installed."));
  expect(screen.getByRole("status").textContent).toBe(
    "FFmpeg is not installed. Install it (for example `brew install ffmpeg`) and scan again.",
  );
});

test("a folder failure keeps the folder message", () => {
  show(folderWith(null, "Music scan failed. Check the connected folder."));
  expect(screen.getByRole("status").textContent).toBe(
    "Scan failed. Check the folder and try again.",
  );
});
