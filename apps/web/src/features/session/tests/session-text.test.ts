import { expect, test } from "vitest";
import { sessionClient, sessionDetail } from "../sessionText";

const base = {
  id: "a".repeat(32),
  created_at: 1_790_000_000,
  current: false,
};

test("a row detail gives last activity then the sign-in time", () => {
  const now = 1_790_100_000_000;
  const session = {
    ...base,
    client: "Firefox on macOS",
    last_seen_at: 1_790_100_000 - 300,
  };
  expect(sessionDetail(session, now)).toMatch(
    /^Last active 5 minutes ago · Signed in .+/,
  );
  expect(
    sessionDetail({ ...session, last_seen_at: 1_790_099_990 }, now),
  ).toMatch(/^Last active just now · Signed in /);
});

test("a session without a label is an unknown browser", () => {
  const session = { ...base, client: null, last_seen_at: 1_790_000_000 };
  expect(sessionClient(session)).toBe("Unknown browser");
  expect(sessionClient({ ...session, client: undefined })).toBe(
    "Unknown browser",
  );
});
