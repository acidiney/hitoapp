import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import PluginSettings from "../components/PluginSettings";
import { usePlugins } from "../hooks/usePlugins";
import {
  confirmedInstallation,
  installationAccount,
  installationDigest,
  installationManifest,
  installationHttp,
  installationToken,
  heldInstallationResponse,
  offeredService,
  otherService,
  replacementInstallationDigest,
  replacementInstallationToken,
  type Persistence,
} from "../fixtures/pluginInstallation";

const logs: unknown[][] = [];

beforeEach(() => {
  logs.length = 0;
  for (const level of ["debug", "info", "warn", "error"] as const)
    vi.spyOn(console, level).mockImplementation((...values) => {
      logs.push(values);
    });
});

function settingsPage(account = installationAccount) {
  return createElement(ArtworkProvider, {
    account,
    children: createElement(PluginSettings, { account, admin: false }),
  });
}

function workflow(policy?: Persistence) {
  const http = installationHttp(policy);
  const view = render(settingsPage());
  return { ...http, view };
}

function expectPrivateLogs(requireInstall = true) {
  const output = JSON.stringify(logs);
  if (requireInstall) expect(output).toContain("plugins.completed");
  expect(logs.length).toBeLessThan(20);
  for (const value of [
    offeredService.name,
    offeredService.base_url,
    installationManifest().id,
    installationManifest().name,
    installationDigest,
    installationToken,
    replacementInstallationToken,
    replacementInstallationDigest,
  ])
    expect(output).not.toContain(value);
}

test.each<{ policy?: Persistence; disclosure: RegExp }>([
  {
    policy: "playback_only",
    disclosure:
      /Playback only\. No new offline downloads\. Completed copies are retained\./,
  },
  {
    policy: "explicit_download",
    disclosure:
      /Explicit downloads declared, subject to Hito's checks\. This does not grant a licence\./,
  },
  {
    disclosure:
      /No persistence declaration\. Download permission is unknown; Hito's checks still apply\./,
  },
])(
  "reviews the $policy declaration before installing with the exact accepted digest",
  async ({ policy, disclosure }) => {
    const { requests } = workflow(policy);
    fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
      target: { value: installationToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
    const heading = await screen.findByRole("heading", {
      name: "Review connection",
    });
    const review = within(heading.parentElement!);
    expect(review.getByText(installationManifest().name)).toBeTruthy();
    expect(review.getByText("search, sources")).toBeTruthy();
    expect(review.getByText(disclosure)).toBeTruthy();
    expect(requests).toEqual([
      { path: "/api/v1/plugins", method: "GET", body: undefined },
      {
        path: "/api/v1/plugins/preview",
        method: "POST",
        body: {
          service_id: offeredService.id,
          service_revision: offeredService.revision,
          bearer_token: installationToken,
        },
      },
    ]);
    expect(
      screen.queryByRole("button", { name: "Check connection" }),
    ).toBeNull();

    fireEvent.click(review.getByRole("button", { name: "Accept and connect" }));
    await screen.findByRole("button", { name: "Check connection" });
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Review connection" }),
      ).toBeNull(),
    );
    expect(requests[2]).toEqual({
      path: "/api/v1/plugins",
      method: "POST",
      body: {
        service_id: offeredService.id,
        service_revision: offeredService.revision,
        bearer_token: installationToken,
        accept_manifest_digest: installationDigest,
      },
    });
    expect(
      (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
        .value,
    ).toBe("");
    expect(document.querySelector("audio")).toBeNull();
    expectPrivateLogs();
  },
);

async function reviewConnection(token?: string) {
  if (token !== undefined)
    fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
      target: { value: token },
    });
  fireEvent.click(
    await screen.findByRole("button", { name: "Review connection" }),
  );
  const heading = await screen.findByRole("heading", {
    name: "Review connection",
  });
  return within(heading.parentElement!);
}

function installs(requests: ReturnType<typeof installationHttp>["requests"]) {
  return requests.filter(
    ({ path, method }) => path === "/api/v1/plugins" && method === "POST",
  );
}

test("going back declines the declaration without installing or persisting its credential draft", async () => {
  const http = workflow("playback_only");
  const review = await reviewConnection(installationToken);
  fireEvent.click(review.getByRole("button", { name: "Back" }));
  expect(
    screen.queryByRole("heading", { name: "Review connection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(installs(http.requests)).toEqual([]);
  expect(http.requests).toHaveLength(2);
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe(installationToken);
  expect(JSON.stringify(localStorage)).not.toContain(installationToken);
  expect(JSON.stringify(sessionStorage)).not.toContain(installationToken);
  expectPrivateLogs(false);
});

test("review and acceptance work over LAN HTTP when randomUUID is unavailable", async () => {
  const getRandomValues = crypto.getRandomValues.bind(crypto);
  vi.stubGlobal("crypto", { getRandomValues });
  const http = workflow("playback_only");
  const review = await reviewConnection(installationToken);
  expect(installs(http.requests)).toEqual([]);
  fireEvent.click(review.getByRole("button", { name: "Accept and connect" }));
  await screen.findByRole("button", { name: "Check connection" });
  expect(http.requests).toHaveLength(3);
  expect(installs(http.requests)[0].body).toEqual({
    service_id: offeredService.id,
    service_revision: offeredService.revision,
    bearer_token: installationToken,
    accept_manifest_digest: installationDigest,
  });
  expect(JSON.stringify(logs)).toMatch(/"operation_id":"[a-f0-9-]{36}"/);
  expectPrivateLogs();
});

test("the review renders the provider's declaration as text without embedding provider HTML or URLs", async () => {
  const http = workflow("playback_only");
  const name =
    '<img src="https://private-install.invalid/provider-ui" onerror="alert(1)">';
  http.state.preview = {
    ...http.state.preview,
    manifest: { ...http.state.preview.manifest, name },
  };
  const review = await reviewConnection();
  expect(review.getByText(name)).toBeTruthy();
  const heading = screen.getByRole("heading", { name: "Review connection" });
  expect(
    heading.parentElement?.querySelector("img, iframe, script, a, audio"),
  ).toBeNull();
  expect(http.requests).toHaveLength(2);
  expect(installs(http.requests)).toEqual([]);
  expect(JSON.stringify(logs)).not.toContain(name);
  expectPrivateLogs(false);
});

test("changing the token invalidates a reviewed declaration and previews only the current draft", async () => {
  const http = workflow("playback_only");
  await reviewConnection(installationToken);
  fireEvent.change(screen.getByLabelText("Bearer token (optional)"), {
    target: { value: replacementInstallationToken },
  });
  expect(
    screen.queryByRole("heading", { name: "Review connection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(installs(http.requests)).toEqual([]);
  await reviewConnection();
  expect(http.requests).toHaveLength(3);
  expect(http.requests[2].body).toEqual({
    service_id: offeredService.id,
    service_revision: offeredService.revision,
    bearer_token: replacementInstallationToken,
  });
  expect(installs(http.requests)).toEqual([]);
  expectPrivateLogs(false);
});

test("selecting another service discards its predecessor's review and credential", async () => {
  const http = workflow("explicit_download");
  await reviewConnection(installationToken);
  fireEvent.change(screen.getByLabelText("Service"), {
    target: { value: otherService.id },
  });
  expect(
    screen.queryByRole("heading", { name: "Review connection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe("");
  await reviewConnection();
  expect(http.requests).toHaveLength(3);
  expect(http.requests[2].body).toEqual({
    service_id: otherService.id,
    service_revision: otherService.revision,
    bearer_token: null,
  });
  expect(installs(http.requests)).toEqual([]);
  expectPrivateLogs(false);
});

test("a changed service revision invalidates review without removing an unrelated private connection", async () => {
  const http = installationHttp("playback_only");
  const unrelated = {
    ...confirmedInstallation("explicit_download"),
    id: "unrelated-installation",
    service_id: otherService.id,
    manifest: {
      ...installationManifest("explicit_download"),
      name: "Already connected service",
    },
  };
  http.state.installations = [unrelated];
  http.state.respond = ({ path }) =>
    path === "/api/v1/plugins/unrelated-installation/refresh"
      ? Response.json(
          { error: "Connection check unavailable." },
          { status: 503 },
        )
      : undefined;
  render(settingsPage());
  fireEvent.click(
    await screen.findByRole("button", { name: "Check connection" }),
  );
  await screen.findByRole("alert");
  await reviewConnection(installationToken);
  http.state.services = [{ ...offeredService, revision: 4 }, otherService];
  fireEvent.click(screen.getByRole("button", { name: "Reload connections" }));
  await waitFor(() =>
    expect(
      (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
        .value,
    ).toBe(""),
  );
  expect(
    screen.queryByRole("heading", { name: "Review connection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(
    screen.getByRole("heading", { name: unrelated.manifest.name }),
  ).toBeTruthy();
  expect(screen.getByText("Token saved")).toBeTruthy();
  await reviewConnection();
  expect(http.requests).toHaveLength(5);
  expect(http.requests[4].body).toEqual({
    service_id: offeredService.id,
    service_revision: 4,
    bearer_token: null,
  });
  expect(installs(http.requests)).toEqual([]);
  expectPrivateLogs(false);
});

test("an unavailable preview retains the private draft and cannot install until a new review succeeds", async () => {
  const http = workflow("playback_only");
  http.state.respond = ({ path }) =>
    path === "/api/v1/plugins/preview"
      ? Response.json(
          { error: "Service declaration unavailable." },
          { status: 503 },
        )
      : undefined;
  fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
    target: { value: installationToken },
  });
  fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("Service declaration unavailable.");
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(
    screen.queryByRole("heading", { name: "Review connection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe(installationToken);
  expect(http.requests).toHaveLength(2);
  expect(installs(http.requests)).toEqual([]);
  http.state.respond = undefined;
  await reviewConnection();
  expect(http.requests).toHaveLength(3);
  expect(installs(http.requests)).toEqual([]);
  expectPrivateLogs(false);
});

test.each([503, 409, "lost response"] as const)(
  "a rejected install (%s) shows one error and requires a fresh declaration before retry",
  async (status) => {
    const http = workflow("explicit_download");
    const review = await reviewConnection(installationToken);
    http.state.respond = ({ path, method }) => {
      if (path !== "/api/v1/plugins" || method !== "POST") return;
      if (status === "lost response") throw new TypeError("connection lost");
      return Response.json(
        {
          error: "Service declaration changed. Review it again.",
          code: status === 409 ? "version_conflict" : "unavailable",
        },
        { status },
      );
    };
    fireEvent.click(review.getByRole("button", { name: "Accept and connect" }));
    await waitFor(() => expect(screen.getAllByRole("alert")).toHaveLength(1));
    expect(screen.getByRole("alert").textContent).toContain(
      status === "lost response"
        ? "Hito cannot be reached right now. It will keep trying."
        : "Service declaration changed. Review it again.",
    );
    expect(
      screen.queryByRole("heading", { name: "Review connection" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Accept and connect" }),
    ).toBeNull();
    expect(
      (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
        .value,
    ).toBe(installationToken);
    expect(installs(http.requests)).toHaveLength(1);
    expect(installs(http.requests)[0].body).toEqual({
      service_id: offeredService.id,
      service_revision: offeredService.revision,
      bearer_token: installationToken,
      accept_manifest_digest: installationDigest,
    });
    http.state.respond = undefined;
    http.state.preview = {
      manifest: installationManifest("playback_only"),
      digest: replacementInstallationDigest,
    };
    const freshReview = await reviewConnection();
    expect(
      freshReview.getByText(/Playback only\. No new offline downloads/),
    ).toBeTruthy();
    expect(installs(http.requests)).toHaveLength(1);
    fireEvent.click(
      freshReview.getByRole("button", { name: "Accept and connect" }),
    );
    await screen.findByRole("button", { name: "Check connection" });
    expect(http.requests).toHaveLength(5);
    expect(installs(http.requests)).toHaveLength(2);
    expect(installs(http.requests)[1].body).toEqual({
      service_id: offeredService.id,
      service_revision: offeredService.revision,
      bearer_token: installationToken,
      accept_manifest_digest: replacementInstallationDigest,
    });
    expectPrivateLogs();
  },
);

test("fresh previews replace only the failed installation's alert while preserving private connections and its draft", async () => {
  const http = installationHttp("explicit_download");
  const unrelated = {
    ...confirmedInstallation("explicit_download"),
    id: "unrelated-installation",
    service_id: otherService.id,
    manifest: {
      ...installationManifest("explicit_download"),
      name: "private-unrelated-installation-name-sentinel",
    },
  };
  http.state.installations = [unrelated];
  render(settingsPage());
  const review = await reviewConnection(installationToken);
  const installError = "The connection could not be saved.";
  http.state.respond = ({ path, method }) =>
    path === "/api/v1/plugins" && method === "POST"
      ? Response.json({ error: installError }, { status: 503 })
      : undefined;
  fireEvent.click(review.getByRole("button", { name: "Accept and connect" }));
  expect((await screen.findByRole("alert")).textContent).toContain(
    installError,
  );
  expect(installs(http.requests)).toHaveLength(1);
  expect(
    screen.getByRole("heading", { name: unrelated.manifest.name }),
  ).toBeTruthy();
  expect(screen.getByText("Token saved")).toBeTruthy();
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe(installationToken);

  const previewError = "The current declaration is unavailable.";
  http.state.respond = ({ path }) =>
    path === "/api/v1/plugins/preview"
      ? Response.json({ error: previewError }, { status: 503 })
      : undefined;
  fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
  await waitFor(() =>
    expect(
      screen.getAllByRole("alert").map((alert) => alert.textContent),
    ).toEqual([previewError]),
  );
  expect(screen.queryByText(installError)).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Accept and connect" }),
  ).toBeNull();
  expect(installs(http.requests)).toHaveLength(1);
  expect(
    screen.getByRole("heading", { name: unrelated.manifest.name }),
  ).toBeTruthy();
  expect(screen.getByText("Token saved")).toBeTruthy();
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe(installationToken);

  http.state.respond = undefined;
  http.state.preview = {
    manifest: installationManifest("playback_only"),
    digest: replacementInstallationDigest,
  };
  const freshReview = await reviewConnection();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    freshReview.getByText(/Playback only\. No new offline downloads/),
  ).toBeTruthy();
  expect(
    screen.getByRole("heading", { name: unrelated.manifest.name }),
  ).toBeTruthy();
  expect(screen.getByText("Token saved")).toBeTruthy();
  expect(
    (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
      .value,
  ).toBe(installationToken);
  expect(installs(http.requests)).toHaveLength(1);
  fireEvent.click(
    freshReview.getByRole("button", { name: "Accept and connect" }),
  );
  await waitFor(() =>
    expect(
      screen.getAllByRole("button", { name: "Check connection" }),
    ).toHaveLength(2),
  );
  expect(
    screen.getByRole("heading", { name: unrelated.manifest.name }),
  ).toBeTruthy();
  expect(screen.queryByLabelText("Bearer token (optional)")).toBeNull();
  expect(http.requests).toHaveLength(6);
  const draft = {
    service_id: offeredService.id,
    service_revision: offeredService.revision,
    bearer_token: installationToken,
  };
  expect(
    http.requests
      .filter(({ path }) => path === "/api/v1/plugins/preview")
      .map(({ body }) => body),
  ).toEqual([draft, draft, draft]);
  expect(installs(http.requests).map(({ body }) => body)).toEqual([
    { ...draft, accept_manifest_digest: installationDigest },
    { ...draft, accept_manifest_digest: replacementInstallationDigest },
  ]);
  expect(document.querySelector("audio")).toBeNull();
  expect(JSON.stringify(logs)).not.toContain(unrelated.manifest.name);
  expectPrivateLogs();
});

test.each(["token", "service"] as const)(
  "editing the %s cancels a held preview and ignores its late declaration",
  async (change) => {
    const http = workflow("playback_only");
    const held = heldInstallationResponse("headers", http.state.preview);
    let intercepted = false;
    http.state.respond = ({ path }) => {
      if (path !== "/api/v1/plugins/preview" || intercepted) return;
      intercepted = true;
      return held.response;
    };
    fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
      target: { value: installationToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
    await waitFor(() => expect(http.calls).toHaveLength(2));
    const signal = http.calls[1].signal;
    if (change === "token")
      fireEvent.change(screen.getByLabelText("Bearer token (optional)"), {
        target: { value: replacementInstallationToken },
      });
    else
      fireEvent.change(screen.getByLabelText("Service"), {
        target: { value: otherService.id },
      });
    await waitFor(() => expect(signal?.aborted).toBe(true));
    await act(async () => held.release());
    expect(
      screen.queryByRole("heading", { name: "Review connection" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Accept and connect" }),
    ).toBeNull();
    expect(installs(http.requests)).toEqual([]);
    await reviewConnection();
    expect(http.requests).toHaveLength(3);
    expect(http.requests[2].body).toEqual({
      service_id: change === "token" ? offeredService.id : otherService.id,
      service_revision: offeredService.revision,
      bearer_token: change === "token" ? replacementInstallationToken : null,
    });
    expectPrivateLogs(false);
  },
);

const ownerChanges = [
  { change: "account", mode: "headers" },
  { change: "account", mode: "body" },
  { change: "unmount", mode: "headers" },
  { change: "unmount", mode: "body" },
] as const;

test.each(ownerChanges)(
  "a held preview $mode cannot expose a declaration after $change",
  async ({ change, mode }) => {
    const http = workflow("playback_only");
    const held = heldInstallationResponse(mode, http.state.preview);
    http.state.respond = ({ path }) =>
      path === "/api/v1/plugins/preview" ? held.response : undefined;
    fireEvent.change(await screen.findByLabelText("Bearer token (optional)"), {
      target: { value: installationToken },
    });
    fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
    await waitFor(() => expect(http.calls).toHaveLength(2));
    const signal = http.calls[1].signal;
    if (change === "account") {
      http.accounts.add("next-install-listener");
      http.view.rerender(settingsPage("next-install-listener"));
      await screen.findByRole("button", { name: "Review connection" });
    } else http.view.unmount();
    await waitFor(() => expect(signal?.aborted).toBe(true));
    await act(async () => held.release());
    expect(
      screen.queryByRole("heading", { name: "Review connection" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Accept and connect" }),
    ).toBeNull();
    expect(screen.queryByText(installationManifest().name)).toBeNull();
    expect(installs(http.requests)).toEqual([]);
    expect(http.requests).toHaveLength(change === "account" ? 3 : 2);
    if (change === "account")
      expect(
        (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
          .value,
      ).toBe("");
    expectPrivateLogs(false);
  },
);

test.each(ownerChanges)(
  "a held acceptance $mode cannot add private connection state after $change",
  async ({ change, mode }) => {
    const http = workflow("playback_only");
    const review = await reviewConnection(installationToken);
    const held = heldInstallationResponse(
      mode,
      confirmedInstallation("playback_only"),
    );
    http.state.respond = ({ path, method }) =>
      path === "/api/v1/plugins" && method === "POST"
        ? held.response
        : undefined;
    fireEvent.click(review.getByRole("button", { name: "Accept and connect" }));
    await waitFor(() => expect(installs(http.requests)).toHaveLength(1));
    const signal = http.calls[2].signal;
    if (change === "account") {
      http.accounts.add("next-install-listener");
      http.view.rerender(settingsPage("next-install-listener"));
      await screen.findByRole("button", { name: "Review connection" });
    } else http.view.unmount();
    await waitFor(() => expect(signal?.aborted).toBe(true));
    await act(async () => held.release());
    expect(
      screen.queryByRole("heading", { name: installationManifest().name }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Check connection" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Accept and connect" }),
    ).toBeNull();
    expect(http.requests).toHaveLength(change === "account" ? 4 : 3);
    if (change === "account")
      expect(
        (screen.getByLabelText("Bearer token (optional)") as HTMLInputElement)
          .value,
      ).toBe("");
    expect(JSON.stringify(logs)).not.toContain('"event":"plugins.completed"');
    expectPrivateLogs(false);
  },
);

test("a caller-aborted install ignores a response that arrives after cancellation", async () => {
  const http = installationHttp("playback_only");
  const held = heldInstallationResponse(
    "headers",
    confirmedInstallation("playback_only"),
  );
  http.state.respond = ({ path, method }) =>
    path === "/api/v1/plugins" && method === "POST" ? held.response : undefined;
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(ArtworkProvider, { account: installationAccount, children });
  const { result } = renderHook(() => usePlugins(installationAccount), {
    wrapper,
  });
  await waitFor(() => expect(result.current.data).not.toBeNull());
  const controller = new AbortController();
  let installation: ReturnType<typeof result.current.install>;
  act(() => {
    installation = result.current.install(
      {
        service_id: offeredService.id,
        service_revision: offeredService.revision,
        bearer_token: installationToken,
        accept_manifest_digest: installationDigest,
      },
      controller.signal,
    );
  });
  await waitFor(() => expect(installs(http.requests)).toHaveLength(1));
  controller.abort();
  expect(http.calls[1].signal?.aborted).toBe(true);
  await act(async () => {
    held.release();
    expect(await installation!).toBeNull();
  });
  expect(result.current.data?.installations).toEqual([]);
  expect(result.current.pending).toBe(false);
  expect(result.current.error).toBe("");
  expect(http.requests).toHaveLength(2);
  expect(JSON.stringify(logs)).not.toContain('"event":"plugins.completed"');
  expectPrivateLogs(false);
});
