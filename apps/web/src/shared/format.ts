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
