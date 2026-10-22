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
