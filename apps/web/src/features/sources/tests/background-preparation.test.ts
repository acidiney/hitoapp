import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { isOn } from "../../../test/switch";
import BackgroundPreparation from "../components/BackgroundPreparation";

function stubFetch() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function calls(fetch: ReturnType<typeof stubFetch>) {
  return fetch.mock.calls.map(([url, init]) => [
    String(url),
    init?.method ?? "GET",
    init?.body ?? null,
  ]);
}

const toggle = () =>
  screen.findByRole("switch", { name: "Prepare sources in the background" });

test("the switch shows the saved choice and saves a change at once", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(Response.json({ enabled: false }));
  render(createElement(BackgroundPreparation, { account: "alice" }));
  const control = await toggle();
  expect(isOn(control)).toBe(true);
  fireEvent.click(control);
  await waitFor(() => expect(isOn(control)).toBe(false));
  expect(calls(fetch)).toEqual([
    ["/api/v1/source-preferences/background", "GET", null],
    [
      "/api/v1/source-preferences/background",
      "PUT",
      JSON.stringify({ enabled: false }),
    ],
  ]);
});

test("a failed save keeps the accepted choice and says why", async () => {
  const fetch = stubFetch();
  fetch
    .mockResolvedValueOnce(Response.json({ enabled: true }))
    .mockResolvedValueOnce(
      Response.json(
        { error: "Enter your PIN to unlock this session" },
        { status: 423 },
      ),
    );
  render(createElement(BackgroundPreparation, { account: "alice" }));
  const control = await toggle();
  fireEvent.click(control);
  expect(
    await screen.findByText("Enter your PIN to unlock this session"),
  ).toBeTruthy();
  expect(isOn(control)).toBe(true);
});
