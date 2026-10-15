import { mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const output = fileURLToPath(new URL('../fixtures/generated/', import.meta.url));
mkdirSync(output, { recursive: true });
for (const frequency of [440, 660]) {
  const rate = 16000;
  const samples = rate * 30;
  const buffer = Buffer.alloc(44 + samples * 2);
  buffer.write('RIFF', 0); buffer.writeUInt32LE(buffer.length - 8, 4); buffer.write('WAVEfmt ', 8);
  buffer.writeUInt32LE(16, 16); buffer.writeUInt16LE(1, 20); buffer.writeUInt16LE(1, 22);
  buffer.writeUInt32LE(rate, 24); buffer.writeUInt32LE(rate * 2, 28); buffer.writeUInt16LE(2, 32); buffer.writeUInt16LE(16, 34);
  buffer.write('data', 36); buffer.writeUInt32LE(samples * 2, 40);
  for (let sample = 0; sample < samples; sample++) buffer.writeInt16LE(Math.round(Math.sin(2 * Math.PI * frequency * sample / rate) * 4096), 44 + sample * 2);
  writeFileSync(`${output}/Fixture ${frequency} Hz.wav`, buffer);
}
console.log('Generated two synthetic 30-second WAV fixtures.');
