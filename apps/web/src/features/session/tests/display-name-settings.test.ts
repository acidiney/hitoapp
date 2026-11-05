import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import DisplayNameSettings from "../components/DisplayNameSettings";

afterEach(() => vi.unstubAllGlobals());

function stubFetch() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function view(displayName: string | null, onSaved = vi.fn()) {
  render(
    createElement(DisplayNameSettings, {
      account: "alice",
      username: "alice",
      displayName,
      onSaved,
    }),
  );
  return {
    onSaved,
    field: screen.getByRole("textbox", {
      name: "Display name",
    }) as HTMLInputElement,
    save: () => screen.getByRole("button", { name: "Save display name" }),
  };
}

test("the field starts from the saved name, explains the username, and saving is off until it changes", () => {
  const { field, save } = view("Ana Souza");
  expect(field.value).toBe("Ana Souza");
  expect(screen.getByText(/You still sign in as alice/)).toBeTruthy();
  expect((save() as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(field, { target: { value: "Ana S." } });
  expect((save() as HTMLButtonElement).disabled).toBe(false);
});

test("saving sends the trimmed name once and reports it", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json({
      id: "alice",
      username: "alice",
      admin: false,
      display_name: "Ana 🦉",
    }),
  );
  const { field, save, onSaved } = view(null);
  fireEvent.change(field, { target: { value: "  Ana 🦉  " } });
  fireEvent.click(save());
  await screen.findByText("Display name saved.");
  expect(fetch).toHaveBeenCalledTimes(1);
  const [url, init] = fetch.mock.calls[0];
  expect(String(url)).toBe("/api/v1/account/display-name");
  expect(init?.method).toBe("PUT");
  expect(init?.body).toBe(JSON.stringify({ display_name: "Ana 🦉" }));
  expect(onSaved).toHaveBeenCalledTimes(1);
  expect(field.value).toBe("Ana 🦉");
  expect((save() as HTMLButtonElement).disabled).toBe(true);
});

test("clearing the field removes the display name", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json({
      id: "alice",
      username: "alice",
      admin: false,
      display_name: null,
    }),
  );
  const { field, save, onSaved } = view("Ana");
  fireEvent.change(field, { target: { value: "   " } });
  fireEvent.click(save());
  await screen.findByText("Display name removed. Your username is shown.");
  expect(fetch.mock.calls[0][1]?.body).toBe(
    JSON.stringify({ display_name: null }),
  );
  expect(onSaved).toHaveBeenCalledTimes(1);
  expect(field.value).toBe("");
});

test("a name over 64 characters is refused before anything is sent", () => {
  const fetch = stubFetch();
  const { field, save } = view(null);
  fireEvent.change(field, { target: { value: "🦉".repeat(65) } });
  fireEvent.click(save());
  expect(screen.getByRole("alert").textContent).toContain("64");
  expect(fetch).not.toHaveBeenCalled();
});

test("the server refusal is shown and the typed name is kept", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json(
      { error: "Use 1 to 64 visible characters for the display name" },
      { status: 422 },
    ),
  );
  const { field, save, onSaved } = view(null);
  fireEvent.change(field, { target: { value: "bad​name" } });
  fireEvent.click(save());
  await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toContain("visible characters");
  expect(field.value).toBe("bad​name");
  expect(onSaved).not.toHaveBeenCalled();
  await waitFor(() =>
    expect((save() as HTMLButtonElement).disabled).toBe(false),
  );
});

test("hostile text stays text", async () => {
  const hostile = '<img src=x onerror="window.pwned=1"> & "quotes"';
  const { container } = render(
    createElement(DisplayNameSettings, {
      account: "alice",
      username: "alice",
      displayName: hostile,
      onSaved: vi.fn(),
    }),
  );
  expect(
    (screen.getByRole("textbox", { name: "Display name" }) as HTMLInputElement)
      .value,
  ).toBe(hostile);
  expect(container.querySelector("img")).toBeNull();
});
