import { expect, test } from "vitest";
import { sessionActivity, sessionClient } from "../sessionText";

const base = {
  id: "a".repeat(32),
  created_at: 1_790_000_000,
  current: false,
};

test("a row names the client and how long ago it was last active", () => {
  const now = 1_790_100_000_000;
  const session = {
    ...base,
    client: "Firefox on macOS",
    last_seen_at: 1_790_100_000 - 300,
  };
  expect(sessionActivity(session, now)).toBe(
    "Firefox on macOS · Last active 5 minutes ago",
  );
  expect(
    sessionActivity({ ...session, last_seen_at: 1_790_099_990 }, now),
  ).toBe("Firefox on macOS · Last active just now");
});

test("a session without a label is an unknown browser", () => {
  const session = { ...base, client: null, last_seen_at: 1_790_000_000 };
  expect(sessionClient(session)).toBe("Unknown browser");
  expect(sessionClient({ ...session, client: undefined })).toBe(
    "Unknown browser",
  );
});
