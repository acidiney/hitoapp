import type { Schema } from "../../../shared/schema";
import { fireEvent, screen } from "@testing-library/react";

import { audiobookPages } from "./audiobooks";

export function bookmarkPage(): Schema<"AudiobookPageProps"> {
  const { detail, bookmark, removedChapterBookmark } = audiobookPages();
  return {
    ...detail,
    detail: {
      ...detail.detail,
      bookmarks: [bookmark, removedChapterBookmark],
    },
  };
}

export function enterBookmark(
  page: Schema<"AudiobookPageProps">,
  label: string,
  position = "35.5",
  entry = page.detail.chapters[2].entry_id,
) {
  fireEvent.change(screen.getByRole("combobox", { name: "Chapter" }), {
    target: { value: entry },
  });
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "Position in seconds" }),
    { target: { value: position } },
  );
  fireEvent.change(screen.getByRole("textbox", { name: "Bookmark label" }), {
    target: { value: label },
  });
}

export function bookmarkResult(
  detail: Schema<"AudiobookDetail">,
  batch: Schema<"SyncBatch">,
): Schema<"AudiobookDetail"> {
  const action = batch.operations[0]?.action;
  if (action?.kind !== "audiobook_bookmark_create")
    throw new Error("A bookmark fixture requires an explicit create action");
  return {
    ...detail,
    bookmarks: [
      ...detail.bookmarks,
      {
        id: action.bookmark_id,
        audiobook_id: action.audiobook_id,
        entry_id: action.entry_id,
        recording_id: action.recording_id,
        position_ms: action.position_ms,
        label: action.label,
      },
    ],
  };
}
