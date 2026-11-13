import { afterEach, expect, test } from "vitest";
import {
  clearCredentialRequirementAfterProtectedSuccess,
  credentialMarkVersion,
  reportCredentialRequirement,
  resetCredentialRequirements,
  connectionNeedsCredentials,
  useCredentialRequirements,
} from "../model/credentialRequirements";
import { renderHook } from "@testing-library/react";

const stamp = (installationId: string, installationRevision: number) => ({
  stamp: { installationId, installationRevision },
});

function flags(account: string) {
  return renderHook(() => useCredentialRequirements(account)).result.current;
}

const plugin = (id: string, revision: number) => ({ id, revision });

afterEach(() => resetCredentialRequirements());

test("a fresh protected success on the marked revision clears that connection only and leaves a rejected peer marked", () => {
  reportCredentialRequirement("bob", stamp("a", 3));
  reportCredentialRequirement("bob", stamp("b", 5));
  const version = credentialMarkVersion("bob", "a");
  clearCredentialRequirementAfterProtectedSuccess("bob", "a", 3, version);
  expect(connectionNeedsCredentials(flags("bob"), plugin("a", 3))).toBe(false);
  expect(connectionNeedsCredentials(flags("bob"), plugin("b", 5))).toBe(true);
});

test("a rejection that lands while the successful request is in flight keeps the mark, even at the same revision", () => {
  reportCredentialRequirement("bob", stamp("a", 3));
  const version = credentialMarkVersion("bob", "a");
  reportCredentialRequirement("bob", stamp("a", 3));
  clearCredentialRequirementAfterProtectedSuccess("bob", "a", 3, version);
  expect(connectionNeedsCredentials(flags("bob"), plugin("a", 3))).toBe(true);
});

test("a rejection that arrives after an unmarked request started is not cleared by that request's success", () => {
  const version = credentialMarkVersion("bob", "a");
  expect(version).toBeNull();
  reportCredentialRequirement("bob", stamp("a", 3));
  clearCredentialRequirementAfterProtectedSuccess("bob", "a", 3, version);
  expect(connectionNeedsCredentials(flags("bob"), plugin("a", 3))).toBe(true);
});

test("a success on another account or another revision does not clear the mark", () => {
  reportCredentialRequirement("bob", stamp("a", 3));
  const version = credentialMarkVersion("bob", "a");
  clearCredentialRequirementAfterProtectedSuccess("alice", "a", 3, version);
  clearCredentialRequirementAfterProtectedSuccess("bob", "a", 4, version);
  expect(connectionNeedsCredentials(flags("bob"), plugin("a", 3))).toBe(true);
});
