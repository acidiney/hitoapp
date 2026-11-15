function plural(count: number, noun: string) {
  return `${count.toLocaleString()} ${count === 1 ? noun : `${noun}s`}`;
}

export function libraryDescription({
  total,
  files,
  query,
  availability = null,
}: {
  total: number;
  files: number;
  query: string;
  availability?: string | null;
}) {
  if (availability === "available_or_service")
    return `${plural(total, "track")} with a source`;
  if (availability === "no_source")
    return `${plural(total, "track")} without a source`;
  if (query) return `${plural(total, "track")} matching “${query}”`;
  if (files > total)
    return `${plural(total, "track")} from ${plural(files, "file")}. Copies of one recording count once.`;
  return plural(total, "track");
}
