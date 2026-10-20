import { randomBytes } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

if (process.getuid() !== 10001 || process.argv.length !== 3)
  throw new Error(
    "Owned fixture preparation requires its isolated user and destination",
  );
const directory = process.argv[2];
await mkdir(join(directory, "secrets"), { mode: 0o700 });
await mkdir(join(directory, "music"), { mode: 0o755 });
await writeFile(
  join(directory, "secrets/community-admin"),
  randomBytes(32).toString("hex") + "\n",
  { flag: "wx", mode: 0o600 },
);
const wav = Buffer.alloc(44 + 32_000);
wav.write("RIFF");
wav.writeUInt32LE(wav.length - 8, 4);
wav.write("WAVEfmt ", 8);
wav.writeUInt32LE(16, 16);
wav.writeUInt16LE(1, 20);
wav.writeUInt16LE(1, 22);
wav.writeUInt32LE(16_000, 24);
wav.writeUInt32LE(32_000, 28);
wav.writeUInt16LE(2, 32);
wav.writeUInt16LE(16, 34);
wav.write("data", 36);
wav.writeUInt32LE(32_000, 40);
await writeFile(join(directory, "music/private-ranking-original.wav"), wav, {
  flag: "wx",
  mode: 0o444,
});
console.log("Owned ranking credentials and original fixture prepared");
