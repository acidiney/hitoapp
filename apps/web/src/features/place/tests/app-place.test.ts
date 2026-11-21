import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import AppPlaceSetting from "../components/AppPlaceSetting";
import { placeApi } from "../api/placeApi";

vi.mock("../api/placeApi", () => ({
  placeApi: { offer: vi.fn(), change: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(placeApi.offer).mockReset();
  vi.mocked(placeApi.change).mockReset();
});

test("a browser has no place to change, so the row is not shown", async () => {
  vi.mocked(placeApi.offer).mockResolvedValue({
    changeable: false,
    current: null,
  });
  const { container } = render(
    createElement(AppPlaceSetting, { account: "owner" }),
  );
  await waitFor(() => expect(placeApi.offer).toHaveBeenCalled());
  expect(container.textContent).toBe("");
});

test("the app says it uses this computer and asks before changing it", async () => {
  vi.mocked(placeApi.offer).mockResolvedValue({
    changeable: true,
    current: { kind: "this_computer" },
  });
  vi.mocked(placeApi.change).mockResolvedValue(undefined);
  render(createElement(AppPlaceSetting, { account: "owner" }));
  expect(await screen.findByText("This app uses")).toBeTruthy();
  expect(screen.getByText("This computer")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change…" }));
  const dialog = await screen.findByRole("dialog", {
    name: "Use a different place?",
  });
  expect(dialog.textContent).toContain(
    "The music stops and the first screen opens. Your library on this computer is kept.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(placeApi.change).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Change…" }));
  await screen.findByRole("dialog", { name: "Use a different place?" });
  fireEvent.click(screen.getByRole("button", { name: "Change" }));
  await waitFor(() => expect(placeApi.change).toHaveBeenCalledWith("owner"));
});

test("the app names the server it uses and says the sign-in stays", async () => {
  vi.mocked(placeApi.offer).mockResolvedValue({
    changeable: true,
    current: { kind: "server", origin: "https://music.example.org" },
  });
  render(createElement(AppPlaceSetting, { account: "owner" }));
  expect(await screen.findByText("https://music.example.org")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Change…" }));
  const dialog = await screen.findByRole("dialog", {
    name: "Use a different place?",
  });
  expect(dialog.textContent).toContain(
    "The music stops and the first screen opens. You stay signed in to music.example.org, and your library on this computer is kept.",
  );
});
