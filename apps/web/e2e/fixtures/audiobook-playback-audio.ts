import {
  auditAudioRequests,
  type AudioRequestEvidence,
} from "./next-entry-audio";
import type { AudiobookSeekWitness } from "./audiobook-playback-seek";

export type AudiobookAudioEvidence = AudioRequestEvidence & {
  bytes?: Buffer;
  range?: string;
  requestRange?: string;
  contentLength?: string;
  verifiedAt: number;
  diagnostic: {
    source: number;
    start: number;
    response: number;
    failure: number;
    finish: number;
    time: number;
    receipt: number;
    responseReceipt: number;
    failureReceipt: number;
    finishReceipt: number;
    end: number;
    network: number;
    body: number;
  };
};
export type AudiobookAudioSource = { url: string; bytes: Buffer };

export function verifiedAudiobookAudio(
  capture: AudiobookAudioEvidence,
  original: Buffer,
) {
  const body = capture.bytes;
  if (
    !body ||
    (capture.contentLength !== undefined &&
      (!/^\d+$/.test(capture.contentLength) ||
        Number(capture.contentLength) !== body.length))
  )
    return false;
  if (capture.status === 200)
    return capture.range === undefined && body.equals(original);
  if (capture.status !== 206) return false;
  const response = capture.range?.match(/^bytes (\d+)-(\d+)\/(\d+)$/);
  const request = capture.requestRange?.match(/^bytes=(\d*)-(\d*)$/);
  if (!response || !request || (!request[1] && !request[2])) return false;
  const [start, end, total] = response.slice(1).map(Number);
  if (
    ![start, end, total].every(Number.isSafeInteger) ||
    total !== original.length
  )
    return false;
  let requestedStart: number;
  let requestedEnd: number;
  if (!request[1]) {
    const suffix = Number(request[2]);
    if (!Number.isSafeInteger(suffix) || suffix <= 0) return false;
    requestedStart = Math.max(0, total - suffix);
    requestedEnd = total - 1;
  } else {
    requestedStart = Number(request[1]);
    requestedEnd = request[2] ? Number(request[2]) : total - 1;
    if (![requestedStart, requestedEnd].every(Number.isSafeInteger))
      return false;
    requestedEnd = Math.min(requestedEnd, total - 1);
  }
  return (
    start === requestedStart &&
    end === requestedEnd &&
    start >= 0 &&
    start <= end &&
    end < total &&
    body.equals(original.subarray(start, end + 1))
  );
}

function bookmarkByte(bytes: Buffer, position: number) {
  if (
    bytes.length < 44 ||
    bytes.length > 4_194_304 ||
    bytes.toString("ascii", 0, 4) !== "RIFF" ||
    bytes.toString("ascii", 8, 12) !== "WAVE" ||
    bytes.readUInt32LE(4) !== bytes.length - 8 ||
    !Number.isSafeInteger(position) ||
    position <= 0
  )
    return -1;
  let rate = 0;
  let alignment = 0;
  for (
    let offset = 12, count = 0;
    offset < bytes.length && count < 8;
    count++
  ) {
    if (offset + 8 > bytes.length) return -1;
    const kind = bytes.toString("ascii", offset, offset + 4);
    const size = bytes.readUInt32LE(offset + 4);
    const end = offset + 8 + size;
    if (end + (size % 2) > bytes.length) return -1;
    if (kind === "fmt ") {
      if (
        size !== 16 ||
        bytes.readUInt16LE(offset + 8) !== 1 ||
        bytes.readUInt16LE(offset + 10) !== 1 ||
        bytes.readUInt32LE(offset + 12) !== 16_000 ||
        bytes.readUInt32LE(offset + 16) !== 32_000 ||
        bytes.readUInt16LE(offset + 20) !== 2 ||
        bytes.readUInt16LE(offset + 22) !== 16
      )
        return -1;
      rate = 32_000;
      alignment = 2;
    }
    if (kind === "data") {
      const relative =
        Math.floor((position * rate) / 1000 / alignment) * alignment;
      return rate && alignment && size === 90 * rate && relative < size
        ? offset + 8 + relative
        : -1;
    }
    offset = end + (size % 2);
  }
  return -1;
}

function usableSeek(witness: AudiobookSeekWitness) {
  if (witness.changed || witness.overflow || witness.events.length !== 5)
    return false;
  const [load, metadata, seeking, seeked, completed] = witness.events;
  return (
    witness.events.every(
      (event, index) =>
        event.kind === index &&
        Number.isSafeInteger(event.owner) &&
        event.owner > 0 &&
        event.owner <= 8 &&
        event.owner === load.owner &&
        event.source === witness.source &&
        event.connected &&
        event.active &&
        event.unique &&
        Number.isFinite(event.time) &&
        event.time > 0 &&
        event.trusted &&
        (index === 0 || event.time >= witness.events[index - 1].time),
    ) &&
    metadata.time >= load.time &&
    seeking.time >= metadata.time &&
    Math.abs(seeking.position - witness.position) <= 25 &&
    Math.abs(seeked.position - witness.position) <= 25 &&
    completed.position > witness.position + 10 &&
    completed.position < witness.position + 2000 &&
    completed.time - load.time <= 10_000 &&
    Number.isFinite(witness.observedUntil) &&
    witness.observedUntil >= completed.time &&
    witness.observedUntil - load.time <= 10_000
  );
}

export function auditAudiobookAudio(
  captured: readonly AudiobookAudioEvidence[],
  sources: readonly AudiobookAudioSource[],
  browser: string,
  witnesses: readonly AudiobookSeekWitness[],
) {
  const records = captured.map((capture) => {
    const source = sources.find((source) => source.url === capture.url);
    return {
      ...capture,
      bodyVerified: !!source && verifiedAudiobookAudio(capture, source.bytes),
    };
  });
  const urls = sources.map((source) => source.url);
  const raw = auditAudioRequests(records, urls, browser);
  const paired = new Set<AudiobookAudioEvidence>();
  if (
    browser === "chromium" &&
    records.length <= 16 &&
    witnesses.length === 1 &&
    usableSeek(witnesses[0])
  ) {
    const witness = witnesses[0];
    const source = sources.find((source) => source.url === witness.source);
    const [load, metadata, seeking, seeked, completed] = witness.events;
    if (source) {
      const total = source.bytes.length;
      const target = bookmarkByte(source.bytes, witness.position);
      const candidates = records.filter(
        (record) =>
          record.url === source.url &&
          record.method === "GET" &&
          record.resourceType === "media" &&
          record.failed &&
          record.finished === 0 &&
          !record.bodyVerified &&
          record.bodyBytes === null &&
          !record.bytes &&
          record.bodyReadFailure !== undefined &&
          record.diagnostic.network === 1 &&
          record.diagnostic.body === 5 &&
          record.started > 0 &&
          record.routed > record.started &&
          record.continued > record.routed &&
          record.responded > record.routed &&
          record.responses === 1 &&
          record.status === 206 &&
          !record.headersEmpty &&
          record.audioContentType &&
          record.contentLength === String(total) &&
          record.requestRange === "bytes=0-" &&
          record.range === `bytes 0-${total - 1}/${total}` &&
          record.diagnostic.time >= load.time - 5 &&
          record.diagnostic.time <= metadata.time + 5 &&
          record.diagnostic.responseReceipt >= load.time - 5 &&
          record.diagnostic.responseReceipt <= metadata.time + 5 &&
          record.diagnostic.failureReceipt >= seeking.time - 5 &&
          record.diagnostic.failureReceipt <= witness.observedUntil + 5 &&
          record.diagnostic.end >= seeking.time - 5 &&
          record.diagnostic.end <= witness.observedUntil + 5,
      );
      if (target >= 0 && candidates.length === 1) {
        const cancelled = candidates[0];
        const prior = records.some(
          (record) =>
            record.url === source.url &&
            record.bodyVerified &&
            auditAudioRequests([record], [source.url], browser).violations ===
              0 &&
            !record.failed &&
            record.finished > record.responded &&
            record.finished < cancelled.started &&
            record.verifiedAt > 0 &&
            record.verifiedAt < cancelled.started &&
            record.bytes?.length === total,
        );
        const replacements = records.filter((record) => {
          const range = record.requestRange?.match(/^bytes=(\d+)-$/);
          const start = range ? Number(range[1]) : -1;
          return (
            record.url === source.url &&
            record.method === "GET" &&
            record.resourceType === "media" &&
            record.bodyVerified &&
            !record.failed &&
            record.finished > record.responded &&
            auditAudioRequests([record], [source.url], browser).violations ===
              0 &&
            record.started > cancelled.started &&
            record.routed > record.started &&
            record.continued > record.routed &&
            record.responded > record.routed &&
            record.responses === 1 &&
            record.status === 206 &&
            record.bodyReadFailure === undefined &&
            record.audioContentType &&
            !record.headersEmpty &&
            Number.isSafeInteger(start) &&
            start > 0 &&
            start <= target &&
            target - start < 32_768 &&
            record.range === `bytes ${start}-${total - 1}/${total}` &&
            record.contentLength === String(total - start) &&
            record.diagnostic.time >= seeking.time - 5 &&
            record.diagnostic.time <= seeked.time + 5 &&
            record.diagnostic.responseReceipt >= seeking.time - 5 &&
            record.diagnostic.responseReceipt <= witness.observedUntil + 5 &&
            record.diagnostic.finishReceipt >= seeking.time - 5 &&
            record.diagnostic.finishReceipt <= witness.observedUntil + 5 &&
            record.diagnostic.end >= seeking.time - 5 &&
            record.diagnostic.end <= witness.observedUntil + 5 &&
            cancelled.diagnostic.failureReceipt <=
              record.diagnostic.finishReceipt + 5
          );
        });
        if (prior && replacements.length === 1) paired.add(cancelled);
      }
    }
  }
  const unexplained = auditAudioRequests(
    records.filter((record) => !paired.has(record)),
    urls,
    browser,
  );
  return {
    ...raw,
    explained_seek_cancellations: paired.size,
    unexplained_violations: unexplained.violations,
    requests: records.length,
    status_zero: records.filter((record) => record.status === 0).length,
    status_200: records.filter((record) => record.status === 200).length,
    status_206: records.filter((record) => record.status === 206).length,
    body_failures: records.filter(
      (record) => record.bodyReadFailure !== undefined,
    ).length,
    verified: records.filter((record) => record.bodyVerified).length,
    distinct: new Set(
      records
        .filter((record) => record.bodyVerified)
        .map((record) => record.url),
    ).size,
  };
}
