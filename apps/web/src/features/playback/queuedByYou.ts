const STORAGE_KEY = "hito.queued-by-you";
const LIMIT = 200;

function read(): string[] {
  try {
    const parsed: unknown = JSON.parse(
      window.localStorage.getItem(STORAGE_KEY) ?? "[]",
    );
    return Array.isArray(parsed)
      ? parsed.filter((id): id is string => typeof id === "string")
      : [];
  } catch {
    return [];
  }
}

let remembered = new Set<string>(typeof window === "undefined" ? [] : read());

export function rememberQueuedByYou(entry: string) {
  remembered.add(entry);
  const recent = [...remembered].slice(-LIMIT);
  remembered = new Set(recent);
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(recent));
  } catch {
    void 0;
  }
}

export function wasQueuedByYou(entry: string): boolean {
  return remembered.has(entry);
}

export function queuedByYouCount(): number {
  return remembered.size;
}

export function forgetQueuedByYouForTests() {
  remembered = new Set();
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    void 0;
  }
}
