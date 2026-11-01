export function time(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  return `${Math.floor(seconds / 60)}:${Math.floor(seconds % 60)
    .toString()
    .padStart(2, "0")}`;
}

export function errorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : "Hito could not complete this request.";
}

export function longDuration(seconds: number) {
  const minutes = Math.max(0, Math.round(seconds / 60));
  return minutes >= 60
    ? `${Math.floor(minutes / 60)} h ${minutes % 60} min`
    : `${minutes} min`;
}

function dayNumber(date: Date) {
  return Math.round(
    Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000,
  );
}

export function friendlyTime(
  date: Date,
  now: Date = new Date(),
  locale?: string,
) {
  const clock = date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
  const days = dayNumber(date) - dayNumber(now);
  const name =
    days === 0
      ? "Today"
      : days === 1
        ? "Tomorrow"
        : days === -1
          ? "Yesterday"
          : null;
  if (name) return `${name}, ${clock}`;
  const day = date.toLocaleDateString(locale, {
    day: "numeric",
    month: "short",
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
  return `${day}, ${clock}`;
}

const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];

export function relativeTime(
  unixSeconds: number,
  now: number = Date.now(),
  locale?: string,
) {
  const elapsed = Math.round(now / 1000 - unixSeconds);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  for (const [unit, size] of RELATIVE_UNITS)
    if (Math.abs(elapsed) >= size)
      return formatter.format(-Math.trunc(elapsed / size), unit);
  return "just now";
}
