import { createElement, Fragment, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import Artwork from "./components/Artwork";
import ArtworkStatus from "./components/ArtworkStatus";
import PluginSettings from "./components/plugins/PluginSettings";
import { artworkApprovalFixture } from "./fixtures/artworkApproval";

test("accepting artwork capability retries a cached unavailable cover without reloading the account", async () => {
  const fixture = artworkApprovalFixture();
  const {
    account,
    recording,
    artwork,
    availability,
    enqueue,
    installed,
    digest,
    refreshes,
    acknowledgeApproval,
  } = fixture;
  const fetch = vi.fn(fixture.respond);
  vi.stubGlobal("fetch", fetch);
  const accountPage = (children: ReactNode) =>
    createElement(ArtworkProvider, { account, children });
  const recordingPage = () =>
    createElement(
      Fragment,
      null,
      createElement(Artwork, { recording }),
      createElement(ArtworkStatus, { recording }),
    );
  const { container, rerender } = render(accountPage(recordingPage()));
  const cover = () => container.querySelector(`img[src="${artwork}"]`);
  const requestsFor = (path: string) =>
    fetch.mock.calls.filter(([requested]) => requested === path);

  await act(async () => {
    fireEvent.error(cover()!);
  });
  await screen.findByText("No artwork provider is available for this track.");
  expect(cover()).toBeNull();
  expect(requestsFor(availability)).toHaveLength(1);
  expect(requestsFor(enqueue)).toHaveLength(0);

  rerender(
    accountPage(createElement(PluginSettings, { account, admin: false })),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Check connection" }),
  );
  const accept = await screen.findByRole("button", { name: "Accept changes" });
  expect(refreshes).toEqual([
    {
      revision: installed.revision,
      accept_manifest_digest: null,
      credential: { action: "keep" },
    },
  ]);
  expect(requestsFor(availability)).toHaveLength(1);
  expect(requestsFor(enqueue)).toHaveLength(0);
  fireEvent.click(accept);
  await waitFor(() => expect(refreshes).toHaveLength(2));
  expect(refreshes[1]).toEqual({
    revision: installed.revision,
    accept_manifest_digest: digest,
    credential: { action: "keep" },
  });
  expect(fixture.artworkApproved).toBe(false);
  expect(requestsFor(availability)).toHaveLength(1);
  expect(requestsFor(enqueue)).toHaveLength(0);

  await act(async () => {
    acknowledgeApproval();
  });
  await screen.findByText("Connection checked.");
  expect(fixture.artworkApproved).toBe(true);
  rerender(accountPage(recordingPage()));
  await act(async () => {
    fireEvent.error(cover()!);
  });

  await waitFor(() => expect(requestsFor(availability)).toHaveLength(2));
  await waitFor(() => expect(requestsFor(enqueue)).toHaveLength(1));
  expect(cover()).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
  fireEvent.load(cover()!);
  expect(refreshes).toHaveLength(2);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({
        "x-hito-account": account,
        "x-hito-request": "1",
      }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
});

test("an old account approval cannot refresh the new account's unavailable cover", async () => {
  const fixture = artworkApprovalFixture();
  const { account, other, refresh, refreshes } = fixture;
  const fetch = vi.fn(fixture.respond);
  vi.stubGlobal("fetch", fetch);
  const accountPage = (account: string, children: ReactNode) =>
    createElement(ArtworkProvider, { account, children });
  const recordingPage = () =>
    createElement(
      Fragment,
      null,
      createElement(Artwork, { recording: other.recording }),
      createElement(ArtworkStatus, { recording: other.recording }),
    );
  const { container, rerender } = render(
    accountPage(
      account,
      createElement(PluginSettings, { account, admin: false }),
    ),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Check connection" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Accept changes" }),
  );
  await waitFor(() => expect(refreshes).toHaveLength(2));
  const acceptance = fetch.mock.calls.filter(
    ([path]) => path === refresh,
  )[1][1];
  expect(acceptance?.signal?.aborted).toBe(false);
  expect(fixture.artworkApproved).toBe(false);

  rerender(accountPage(other.account, recordingPage()));
  expect(acceptance?.signal?.aborted).toBe(true);
  const cover = () => container.querySelector(`img[src="${other.artwork}"]`);
  await act(async () => {
    fireEvent.error(cover()!);
  });
  const unavailable = "No artwork provider is available for this track.";
  await screen.findByText(unavailable);
  const otherRequests = () =>
    fetch.mock.calls.filter(
      ([, options]) =>
        new Headers(options?.headers).get("x-hito-account") === other.account,
    );
  expect(otherRequests().map(([path]) => path)).toEqual([other.availability]);
  expect(cover()).toBeNull();

  await act(async () => {
    fixture.acknowledgeApproval();
  });
  expect(fixture.artworkApproved).toBe(true);
  expect(otherRequests().map(([path]) => path)).toEqual([other.availability]);
  expect(cover()).toBeNull();
  expect(screen.getByText(unavailable)).toBeTruthy();
  expect(screen.queryByText("Connection checked.")).toBeNull();
  expect(
    fetch.mock.calls.filter(([path]) => path === other.enqueue),
  ).toHaveLength(0);

  rerender(
    accountPage(other.account, createElement("p", null, "Another page")),
  );
  rerender(accountPage(other.account, recordingPage()));
  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(cover()).toBeNull();
  expect(screen.getByText(unavailable)).toBeTruthy();
  expect(otherRequests().map(([path]) => path)).toEqual([other.availability]);
});
