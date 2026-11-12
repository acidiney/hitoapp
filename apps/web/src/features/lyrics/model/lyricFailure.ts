import { HttpError } from "../../../shared/http";

export type LyricFailureKind =
  | "access"
  | "gone"
  | "conflict"
  | "temporary"
  | "changed"
  | "deadline";

export type LyricFailure = {
  kind: LyricFailureKind;
  message: string;
  reload: boolean;
};

export type LyricStep =
  | "services"
  | "identities"
  | "search"
  | "review"
  | "accept"
  | "remove"
  | "read";

const temporaryMessages: Record<LyricStep, string> = {
  services: "Lyric services could not be loaded. Try again.",
  identities: "Saved lyric choices could not be loaded. Try again.",
  search: "The lyric search could not be completed. Try again.",
  review: "These lyrics could not be loaded for review. Try again.",
  accept: "The lyric choice could not be saved. Try again.",
  remove: "The lyric choice could not be removed. Try again.",
  read: "Lyrics could not be loaded. Try again.",
};

export const accessMessage =
  "Your account access changed. Sign in again to use lyrics.";
export const goneMessage =
  "This track or lyric choice is no longer available. Saved choices were reloaded.";
export const changedMessage =
  "Track details changed. Reload the lyric choices and try again.";

export function deadlineFailure(step: LyricStep): LyricFailure {
  return {
    kind: "deadline",
    message:
      step === "read"
        ? "Lyrics took too long to load. Try again."
        : "This took too long. Try again.",
    reload: false,
  };
}

export function changedFailure(): LyricFailure {
  return { kind: "changed", message: changedMessage, reload: true };
}

export function lyricFailure(error: unknown, step: LyricStep): LyricFailure {
  if (error instanceof HttpError) {
    if ([401, 403, 423].includes(error.status))
      return { kind: "access", message: accessMessage, reload: false };
    if (error.status === 404)
      return { kind: "gone", message: goneMessage, reload: true };
    if (error.status === 409)
      return {
        kind: "conflict",
        message: error.message,
        reload: true,
      };
  }
  return { kind: "temporary", message: temporaryMessages[step], reload: false };
}
