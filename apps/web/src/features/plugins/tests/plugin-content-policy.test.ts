import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { findCheckConnection } from "../fixtures/pluginInstallation";
import { beforeEach, expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import PluginSettings from "../components/PluginSettings";

vi.mock("../components/PluginPlaybackOrder", () => ({ default: () => null }));
type Persistence = NonNullable<
  Schema<"PluginInstallation">["manifest"]["content_policy"]
>["source_persistence"];

const account = "policy-listener";
const digest = "private-policy-reviewed-digest-sentinel";
const replacementToken = "private-policy-replacement-token-sentinel";
const service = {
  id: "policy-service",
  name: "private-policy-service-name-sentinel",
  base_url: "https://private-policy-service.invalid/private-path/",
  revision: 1,
};
const refreshPath = "/api/v1/plugins/policy-installation/refresh";
const logs: unknown[][] = [];

beforeEach(() => {
  logs.length = 0;
  for (const level of ["debug", "info", "warn", "error"] as const)
    vi.spyOn(console, level).mockImplementation((...values) => {
      logs.push(values);
    });
});

function installation(policy?: Persistence): Schema<"PluginInstallation"> {
  return {
    id: "policy-installation",
    service_id: service.id,
    revision: 1,
    checked_at: 1,
    credential_set: true,
    manifest: {
      id: "org.hito.private-policy-manifest-sentinel",
      name: "private-policy-provider-name-sentinel",
      protocol_version: 1,
      capabilities: ["search", "sources"],
      ...(policy ? { content_policy: { source_persistence: policy } } : {}),
    },
  };
}

function workflow(current?: Persistence, proposed?: Persistence) {
  let accepted = installation(current);
  const manifest = installation(proposed).manifest;
  const requests: {
    path: string;
    method: string;
    body?: Schema<"RefreshPlugin">;
  }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const path = String(input);
      const method = options?.method ?? "GET";
      const body = options?.body
        ? (JSON.parse(String(options.body)) as Schema<"RefreshPlugin">)
        : undefined;
      if (path.endsWith("/declaration-check"))
        return Response.json({ kind: "current" });
      requests.push({ path, method, body });
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(account);
      if (path === "/api/v1/plugins" && method === "GET")
        return Response.json({
          credential_storage_ready: true,
          services: [service],
          installations: [accepted],
        });
      if (path === refreshPath && method === "POST") {
        if (body?.accept_manifest_digest === digest) {
          accepted = { ...accepted, revision: 2, manifest };
          return Response.json({ kind: "updated", installation: accepted });
        }
        return Response.json({ kind: "review_required", manifest, digest });
      }
      return Response.json({ error: "Unexpected request" }, { status: 500 });
    }),
  );
  render(
    createElement(ArtworkProvider, {
      account,
      children: createElement(PluginSettings, { account, admin: false }),
    }),
  );
  return { requests, accepted: () => accepted };
}

function acceptedDisclosure() {
  const term = screen.getByText("Source persistence");
  expect(term.tagName).toBe("DT");
  return term.nextElementSibling as Element;
}

function reviewDisclosure(label: "Current" | "Proposed") {
  const term = screen.getByText(
    label === "Current" ? "Saving audio today" : "Saving audio if you accept",
  );
  expect(term.tagName).toBe("DT");
  const definition = term.nextElementSibling;
  expect(definition?.tagName).toBe("DD");
  return definition!;
}

function expectPolicy(element: Element, policy?: Persistence) {
  const text = element.textContent ?? "";
  if (policy === "playback_only") {
    expect(text).toMatch(/only plays audio/i);
    expect(text).toMatch(/won't save new copies/i);
    expect(text).toMatch(/copies you already have stay/i);
  } else if (policy === "explicit_download") {
    expect(text).toMatch(/allows saving audio when you ask/i);
    expect(text).toMatch(/still checks before saving/i);
    expect(text).toMatch(/isn't a licen[cs]e/i);
  } else {
    expect(text).toMatch(/doesn't say whether its audio may be saved/i);
    expect(text).toMatch(/still asks before saving/i);
    expect(text).not.toMatch(/allows saving audio|only plays audio/i);
  }
}

function expectPrivateLogs() {
  const output = JSON.stringify(logs);
  expect(output).toContain("plugins.completed");
  for (const privateValue of [
    service.name,
    service.base_url,
    installation().manifest.id,
    installation().manifest.name,
    digest,
    replacementToken,
  ])
    expect(output).not.toContain(privateValue);
}

const transitions: {
  name: string;
  current?: Persistence;
  proposed?: Persistence;
}[] = [
  { name: "undeclared to playback only", proposed: "playback_only" },
  {
    name: "explicit downloads to playback only",
    current: "explicit_download",
    proposed: "playback_only",
  },
  { name: "playback only to undeclared", current: "playback_only" },
  {
    name: "playback only to explicit downloads",
    current: "playback_only",
    proposed: "explicit_download",
  },
  { name: "undeclared to explicit downloads", proposed: "explicit_download" },
  { name: "explicit downloads to undeclared", current: "explicit_download" },
];

test.each(transitions)(
  "policy-only review discloses $name without changing the accepted declaration before acceptance",
  async ({ current, proposed }) => {
    const fixture = workflow(current, proposed);
    const check = await findCheckConnection();
    expectPolicy(acceptedDisclosure(), current);
    fireEvent.click(check);
    await screen.findByRole("dialog");
    expectPolicy(acceptedDisclosure(), current);
    expectPolicy(reviewDisclosure("Current"), current);
    expectPolicy(reviewDisclosure("Proposed"), proposed);
    expect(fixture.accepted()).toEqual(installation(current));

    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expectPolicy(acceptedDisclosure(), current);
    expect(fixture.accepted()).toEqual(installation(current));
    expect(fixture.requests).toHaveLength(2);

    fireEvent.click(check);
    await screen.findByRole("dialog");
    expectPolicy(acceptedDisclosure(), current);
    expectPolicy(reviewDisclosure("Proposed"), proposed);
    fireEvent.click(screen.getByRole("button", { name: "Accept changes" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expectPolicy(acceptedDisclosure(), proposed);
    expect(fixture.requests).toEqual([
      { path: "/api/v1/plugins", method: "GET", body: undefined },
      ...[null, null, digest].map((accepted) => ({
        path: refreshPath,
        method: "POST",
        body: {
          revision: 1,
          accept_manifest_digest: accepted,
          credential: { action: "keep" },
        },
      })),
    ]);
    expect(document.querySelector("audio")).toBeNull();
    expectPrivateLogs();
  },
);

test("declining a policy review clears a private replacement-token draft without another request", async () => {
  const fixture = workflow("explicit_download", "playback_only");
  await findCheckConnection();
  const input = screen.getByLabelText("New bearer token") as HTMLInputElement;
  fireEvent.change(input, { target: { value: replacementToken } });
  fireEvent.click(screen.getByRole("button", { name: "Save token" }));
  await screen.findByRole("dialog");
  expectPolicy(acceptedDisclosure(), "explicit_download");
  expectPolicy(reviewDisclosure("Current"), "explicit_download");
  expectPolicy(reviewDisclosure("Proposed"), "playback_only");
  expect(fixture.requests[1]).toEqual({
    path: refreshPath,
    method: "POST",
    body: {
      revision: 1,
      accept_manifest_digest: null,
      credential: { action: "replace", bearer_token: replacementToken },
    },
  });

  fireEvent.click(screen.getByRole("button", { name: "Not now" }));
  expect(input.value).toBe("");
  expectPolicy(acceptedDisclosure(), "explicit_download");
  expect(fixture.accepted()).toEqual(installation("explicit_download"));
  expect(fixture.requests.map(({ path }) => path)).toEqual([
    "/api/v1/plugins",
    refreshPath,
  ]);
  expectPrivateLogs();
});
