export type ShortcutCommand =
  | "help"
  | "togglePlay"
  | "next"
  | "previous"
  | "mute"
  | "like"
  | "queue"
  | "nowPlaying"
  | "home"
  | "library"
  | "plugins";

export type ShortcutState = { chord: boolean; singleKeys: boolean };

const GO_TO: Record<string, ShortcutCommand> = {
  h: "home",
  l: "library",
  p: "plugins",
};
const LETTERS: Record<string, ShortcutCommand> = {
  m: "mute",
  l: "like",
  q: "queue",
  n: "nowPlaying",
};
const OWN_SPACE =
  "button, a[href], input, select, textarea, summary, [role='button'], [role='slider'], [role='switch'], [role='menuitem'], [role='option'], [role='tab']";

function typing(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.getAttribute("contenteditable") === "true" ||
      target.matches("input:not([type='range']), textarea, select"))
  );
}

let pointerControl: Element | null = null;

export function notePointer(target: EventTarget | null) {
  pointerControl = target instanceof Element ? target.closest(OWN_SPACE) : null;
}

function ignored(event: KeyboardEvent): boolean {
  return (
    event.defaultPrevented ||
    event.isComposing ||
    event.metaKey ||
    event.ctrlKey ||
    event.altKey ||
    typing(event.target) ||
    document.querySelector("dialog[open], [aria-modal='true']") !== null
  );
}

export function resolveShortcut(
  event: KeyboardEvent,
  state: ShortcutState,
): ShortcutCommand | null {
  if (event.key === "Tab") pointerControl = null;
  if (ignored(event)) return null;
  if (event.key === "?") return "help";
  if (event.shiftKey) {
    if (event.key === "ArrowRight") return "next";
    if (event.key === "ArrowLeft") return "previous";
    return null;
  }
  if (!state.singleKeys) return null;
  const key = event.key.toLowerCase();
  if (state.chord && GO_TO[key]) return GO_TO[key];
  if (key === " ") {
    const own =
      event.target instanceof Element ? event.target.closest(OWN_SPACE) : null;
    return own && own !== pointerControl ? null : "togglePlay";
  }
  return LETTERS[key] ?? null;
}

export function nextChord(event: KeyboardEvent, state: ShortcutState): boolean {
  return (
    state.singleKeys &&
    !state.chord &&
    !ignored(event) &&
    !event.shiftKey &&
    event.key.toLowerCase() === "g"
  );
}
