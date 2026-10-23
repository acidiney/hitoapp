import { readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const serverLimit = 10;
const serverWindowMs = 60_000;
const margin = 500;
const record = join(tmpdir(), "hito-e2e-auth-attempts.json");

async function recorded(): Promise<number[]> {
  try {
    return JSON.parse(await readFile(record, "utf8")) as number[];
  } catch {
    return [];
  }
}

/**
 * The server allows ten sign-in or setup attempts per minute from one address,
 * and every spec reaches it from the same address. Waiting for the window to
 * open keeps sequential specs inside that documented limit instead of making
 * the later ones fail.
 */
export async function paceAuthAttempt() {
  for (;;) {
    const now = Date.now();
    const recent = (await recorded()).filter(
      (time) => now - time < serverWindowMs + margin,
    );
    if (recent.length < serverLimit) {
      await writeFile(record, JSON.stringify([...recent, now]));
      return;
    }
    await new Promise((resolve) =>
      setTimeout(resolve, recent[0] + serverWindowMs + margin - now),
    );
  }
}
