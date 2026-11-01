const PROBE_TIMEOUT_MS = 4000;

export async function sourceWasDelivered(path: string): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const response = await fetch(path, {
      headers: { Range: "bytes=0-0" },
      signal: controller.signal,
    });
    controller.abort();
    const type = response.headers.get("content-type") ?? "";
    return (
      response.ok &&
      (type.startsWith("audio/") || type.startsWith("application/octet-stream"))
    );
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}
