import { expect, test } from "vitest";
import {
  audioDetail,
  audioSummary,
  lastPlayedText,
  trackPositionText,
} from "../model/trackFacts";

const flac = {
  format: "FLAC",
  bit_depth: 16,
  sample_rate_hz: 44100,
  size_bytes: 31_400_000,
};

test("a lossless file reads as codec, depth and rate in the header and with units in the overview", () => {
  expect(audioSummary(flac)).toBe("FLAC 16/44.1");
  expect(audioDetail(flac)).toBe("FLAC 16-bit / 44.1 kHz · 31.4 MB");
});

test("unknown parts are left out rather than guessed", () => {
  const lossy = { ...flac, format: "MP3", bit_depth: null };
  expect(audioSummary(lossy)).toBe("MP3 44.1 kHz");
  expect(audioDetail(lossy)).toBe("MP3 44.1 kHz · 31.4 MB");
  const sizeOnly = {
    format: null,
    bit_depth: null,
    sample_rate_hz: null,
    size_bytes: 2_500,
  };
  expect(audioSummary(sizeOnly)).toBeNull();
  expect(audioDetail(sizeOnly)).toBe("2.5 KB");
  expect(audioSummary(null)).toBeNull();
  expect(audioDetail(null)).toBeNull();
});

test("sample rates keep their decimals and sizes switch units", () => {
  expect(audioSummary({ ...flac, sample_rate_hz: 96000, bit_depth: 24 })).toBe(
    "FLAC 24/96",
  );
  expect(audioSummary({ ...flac, sample_rate_hz: 88200 })).toBe("FLAC 16/88.2");
  expect(audioDetail({ ...flac, size_bytes: 1_250_000_000 })).toContain(
    "1.25 GB",
  );
});

test("last played names the time and the resume position, and is absent when never played", () => {
  const now = Date.UTC(2026, 10, 6, 12) / 1000;
  const twoDays = now - 2 * 86_400;
  expect(
    lastPlayedText({ played_at: twoDays, position_ms: 97_000 }, now * 1000),
  ).toBe("2 days ago · stopped at 1:37");
  expect(
    lastPlayedText({ played_at: twoDays, position_ms: 0 }, now * 1000),
  ).toBe("2 days ago");
  expect(lastPlayedText(null, now * 1000)).toBeNull();
});

test("track position only uses what is known", () => {
  expect(trackPositionText(3, 1, 11)).toBe("Track 3 of 11");
  expect(trackPositionText(3, 1, null)).toBe("Track 3");
  expect(trackPositionText(3, 2, 11)).toBe("Disc 2 · Track 3 of 11");
  expect(trackPositionText(null, 1, 11)).toBeNull();
});
