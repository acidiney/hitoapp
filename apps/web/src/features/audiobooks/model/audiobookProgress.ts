import type { AudiobookDetail } from "../../../shared/types";

export type AudiobookProgress = {
  chapterNumber: number;
  overall: { percent: number; leftMs: number } | null;
};

/**
 * Position in the book: the continuing chapter's saved position plus the full
 * length of the chapters before it. The overall figure needs a known duration
 * for every chapter.
 */
export function audiobookProgress(
  detail: AudiobookDetail,
): AudiobookProgress | null {
  const continuation = detail.continuation;
  if (!continuation?.progress) return null;
  const index = detail.chapters.findIndex(
    (chapter) => chapter.entry_id === continuation.chapter.entry_id,
  );
  if (index < 0) return null;
  const chapterNumber = index + 1;
  if (detail.chapters.some((chapter) => chapter.duration_ms <= 0))
    return { chapterNumber, overall: null };
  const total = detail.chapters.reduce(
    (sum, chapter) => sum + chapter.duration_ms,
    0,
  );
  const before = detail.chapters
    .slice(0, index)
    .reduce((sum, chapter) => sum + chapter.duration_ms, 0);
  const position = Math.min(
    continuation.progress.position_ms,
    detail.chapters[index].duration_ms,
  );
  const done = before + position;
  return {
    chapterNumber,
    overall: {
      percent: (done / total) * 100,
      leftMs: Math.max(0, total - done),
    },
  };
}
