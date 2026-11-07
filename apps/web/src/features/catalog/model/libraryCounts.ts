function plural(count: number, noun: string) {
  return `${count.toLocaleString()} ${count === 1 ? noun : `${noun}s`}`;
}

export function libraryDescription({
  total,
  files,
  query,
}: {
  total: number;
  files: number;
  query: string;
}) {
  if (query) return `${plural(total, "track")} matching “${query}”`;
  if (files > total)
    return `${plural(total, "track")} from ${plural(files, "file")}. Copies of one recording count once.`;
  return plural(total, "track");
}
