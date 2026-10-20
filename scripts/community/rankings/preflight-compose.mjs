import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";

async function writerReceipt() {
  try {
    const value = JSON.parse(
      await readFile("/evidence/compose-writer.json", "utf8"),
    );
    if (
      value.file === "compose" &&
      Number.isInteger(value.exit) &&
      value.exit >= 0 &&
      value.exit <= 255 &&
      Number.isSafeInteger(value.bytes) &&
      value.bytes >= 0 &&
      typeof value.sha256 === "string" &&
      /^[0-9a-f]{64}$/.test(value.sha256)
    )
      return {
        exit: value.exit,
        bytes: value.bytes,
        sha256: value.sha256,
      };
  } catch {}
  return null;
}

export async function readComposeEvidence() {
  const bytes = await readFile("/evidence/compose.json");
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    const proof = {
      file: "compose",
      readBytes: bytes.length,
      readSha256: createHash("sha256").update(bytes).digest("hex"),
      parseFailure:
        error instanceof SyntaxError &&
        error.message.includes("Unexpected end of JSON input")
          ? "unexpected_eof"
          : "invalid_json",
      writer: await writerReceipt(),
    };
    try {
      await writeFile(
        "/evidence/preflight-compose-failure.json",
        JSON.stringify(proof) + "\n",
        { flag: "wx", mode: 0o600 },
      );
    } catch {
      console.error(
        JSON.stringify({
          event: "preflight_failure_receipt_unwritten",
          ...proof,
        }),
      );
    }
    const failure = new Error("Owned preflight compose JSON parse failed");
    failure.stack = failure.message;
    throw failure;
  }
}
