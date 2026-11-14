import { mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const arguments_ = process.argv.slice(2);
if (arguments_.length !== new Set(arguments_).size ||
    arguments_.some(argument => !['--e2e', '--audiobook'].includes(argument)) ||
    (arguments_.includes('--audiobook') && !arguments_.includes('--e2e'))) {
  throw new Error('Usage: audio-fixtures.mjs [--e2e [--audiobook]]');
}
const folder = arguments_.includes('--e2e') ? '../fixtures/e2e-generated/' : '../fixtures/generated/';
const output = fileURLToPath(new URL(folder, import.meta.url));

function tone(frequency, rate, samples) {
  const buffer = Buffer.alloc(44 + samples * 2);
  buffer.write('RIFF', 0); buffer.writeUInt32LE(buffer.length - 8, 4); buffer.write('WAVEfmt ', 8);
  buffer.writeUInt32LE(16, 16); buffer.writeUInt16LE(1, 20); buffer.writeUInt16LE(1, 22);
  buffer.writeUInt32LE(rate, 24); buffer.writeUInt32LE(rate * 2, 28); buffer.writeUInt16LE(2, 32); buffer.writeUInt16LE(16, 34);
  buffer.write('data', 36); buffer.writeUInt32LE(samples * 2, 40);
  for (let sample = 0; sample < samples; sample++) buffer.writeInt16LE(Math.round(Math.sin(2 * Math.PI * frequency * sample / rate) * 4096), 44 + sample * 2);
  return buffer;
}
function infoList(fields) {
  const tags = fields.map(([key, value]) => {
    const text = Buffer.from(`${value}\0`);
    const chunk = Buffer.alloc(8 + text.length + text.length % 2);
    chunk.write(key); chunk.writeUInt32LE(text.length, 4); text.copy(chunk, 8);
    return chunk;
  });
  const list = Buffer.concat([Buffer.from('LIST\0\0\0\0INFO', 'binary'), ...tags]);
  list.writeUInt32LE(list.length - 8, 4);
  return list;
}
function synchsafe(value) {
  return Buffer.from([(value >> 21) & 0x7f, (value >> 14) & 0x7f, (value >> 7) & 0x7f, value & 0x7f]);
}
function id3Frame(identifier, payload) {
  return Buffer.concat([Buffer.from(identifier, 'latin1'), synchsafe(payload.length), Buffer.from([0, 0]), payload]);
}
function id3Chunk(title, artist, artists) {
  const frames = Buffer.concat([
    id3Frame('TIT2', Buffer.concat([Buffer.from([3]), Buffer.from(title)])),
    id3Frame('TPE1', Buffer.concat([Buffer.from([3]), Buffer.from(artist)])),
    id3Frame('TXXX', Buffer.concat([Buffer.from([3]), Buffer.from('ARTISTS\0'), Buffer.from(artists.join('\0'))])),
  ]);
  const tag = Buffer.concat([Buffer.from('ID3'), Buffer.from([4, 0, 0]), synchsafe(frames.length), frames]);
  const chunk = Buffer.alloc(8 + tag.length + tag.length % 2);
  chunk.write('ID3 '); chunk.writeUInt32LE(tag.length, 4); tag.copy(chunk, 8);
  return chunk;
}
mkdirSync(output, { recursive: true });
for (const frequency of [440, 660]) {
  const destination = arguments_.includes('--e2e') ? `${output}/${frequency === 440 ? 'Music' : 'Archive'}` : output;
  mkdirSync(destination, { recursive: true });
  const rate = 16000;
  const seconds = arguments_.includes('--e2e') ? 90 : 30;
  const samples = rate * seconds;
  const buffer = tone(frequency, rate, samples);
  const fields = [['INAM', `Fixture ${frequency} Hz`], ['IART', 'Hito test fixtures'], ['IPRD', `Fixture collection ${frequency} Hz`]];
  if (arguments_.includes('--e2e')) fields.push(['IGNR', 'Fixture tone']);
  const list = infoList(fields);
  const audio = Buffer.concat(arguments_.includes('--audiobook')
    ? [buffer.subarray(0, 36), list, buffer.subarray(36)]
    : [buffer, list]);
  audio.writeUInt32LE(audio.length - 8, 4);
  writeFileSync(`${destination}/Fixture ${frequency} Hz.wav`, audio);
}
if (arguments_.includes('--e2e')) {
  const credits = `${output}/Credits`;
  mkdirSync(credits, { recursive: true });
  const title = 'Shared credit members fixture';
  const artists = ['Shared Lead Credit', 'Shared Guest Credit'];
  const display = `${artists[0]} feat. ${artists[1]}`;
  const audio = Buffer.concat([
    tone(523, 16000, 16000 * 20),
    infoList([['INAM', title], ['IART', display], ['IPRD', 'Shared credit members collection']]),
    id3Chunk(title, display, artists),
  ]);
  audio.writeUInt32LE(audio.length - 8, 4);
  writeFileSync(`${credits}/Shared credit members fixture.wav`, audio);
}
if (arguments_.includes('--e2e')) writeFileSync(`${output}/storage.json`, JSON.stringify({ version: 1, directories: [
  { id: 'fixture-music', name: 'Music', root: '/music/Music' },
  { id: 'fixture-archive', name: 'Archive', root: '/music/Archive' },
  { id: 'fixture-credits', name: 'Credits', root: '/music/Credits' },
] }));
console.log('Generated two synthetic WAV fixtures.');
