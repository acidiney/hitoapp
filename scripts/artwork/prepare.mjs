import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const directory = process.argv[2];
if (!directory)
  throw new Error("An owned synthetic music directory is required");
await mkdir(directory, { recursive: true });
const rate = 16000;
const frames = rate;
const audio = Buffer.alloc(44 + frames * 2);
audio.write("RIFF", 0);
audio.writeUInt32LE(audio.length - 8, 4);
audio.write("WAVEfmt ", 8);
audio.writeUInt32LE(16, 16);
audio.writeUInt16LE(1, 20);
audio.writeUInt16LE(1, 22);
audio.writeUInt32LE(rate, 24);
audio.writeUInt32LE(rate * 2, 28);
audio.writeUInt16LE(2, 32);
audio.writeUInt16LE(16, 34);
audio.write("data", 36);
audio.writeUInt32LE(frames * 2, 40);
for (let frame = 0; frame < frames; frame++)
  audio.writeInt16LE(
    Math.round(Math.sin((2 * Math.PI * 440 * frame) / rate) * 4096),
    44 + frame * 2,
  );
await writeFile(join(directory, "owned-original.wav"), audio, { mode: 0o444 });
console.log("Owned synthetic original prepared");
