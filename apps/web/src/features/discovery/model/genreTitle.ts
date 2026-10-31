const ACRONYMS = new Set(["r&b", "edm", "uk", "us", "dj", "mpb"]);

const titleWord = (word: string): string =>
  ACRONYMS.has(word)
    ? word.toUpperCase()
    : word.charAt(0).toUpperCase() + word.slice(1);

/** Display form of a genre label; the genre key used in URLs is never changed. */
export function genreTitle(label: string): string {
  if (/\p{Lu}/u.test(label)) return label;
  return label
    .split(/([ -])/)
    .map((part) => (part === " " || part === "-" ? part : titleWord(part)))
    .join("");
}
