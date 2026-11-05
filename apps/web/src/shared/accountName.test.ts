import { expect, test } from "vitest";
import { accountName } from "./accountName";

test("the display name wins and the username is the fallback", () => {
  expect(accountName({ username: "alice", display_name: "Alice A." })).toBe(
    "Alice A.",
  );
  expect(accountName({ username: "alice", display_name: null })).toBe("alice");
  expect(accountName({ username: "alice" })).toBe("alice");
  expect(accountName({ username: "alice", display_name: "" })).toBe("alice");
});
