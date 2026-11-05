import { createElement } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AvatarSettings from "../components/AvatarSettings";

afterEach(() => vi.unstubAllGlobals());

function stubFetch() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function view(hasPhoto: boolean, onSaved = vi.fn()) {
  render(
    createElement(AvatarSettings, { account: "alice", hasPhoto, onSaved }),
  );
  return {
    onSaved,
    file: screen.getByLabelText("Profile photo file") as HTMLInputElement,
  };
}

function choose(input: HTMLInputElement, file: File) {
  fireEvent.change(input, { target: { files: [file] } });
}

test("remove is offered only while a photo exists", () => {
  view(false);
  expect(screen.getByRole("button", { name: "Upload photo" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Remove photo" })).toBeNull();
});

test("a chosen picture is sent once as raw bytes and reported", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json({
      id: "alice",
      username: "alice",
      admin: false,
      avatar_version: "v1",
    }),
  );
  const { file, onSaved } = view(false);
  const picture = new File([new Uint8Array([1, 2, 3])], "me.png", {
    type: "image/png",
  });
  choose(file, picture);
  await screen.findByText("Photo saved.");
  expect(fetch).toHaveBeenCalledTimes(1);
  const [url, init] = fetch.mock.calls[0];
  expect(String(url)).toBe("/api/v1/account/avatar");
  expect(init?.method).toBe("PUT");
  expect(init?.body).toBe(picture);
  expect(new Headers(init?.headers).get("content-type")).toBe(
    "application/octet-stream",
  );
  expect(onSaved).toHaveBeenCalledTimes(1);
});

test("pictures over 2 MiB or of another type are refused before any request", () => {
  const fetch = stubFetch();
  const { file, onSaved } = view(false);
  choose(
    file,
    new File([new Uint8Array(2 * 1024 * 1024 + 1)], "big.jpg", {
      type: "image/jpeg",
    }),
  );
  expect(screen.getByRole("alert").textContent).toBe(
    "Use a picture of 2 MiB or less.",
  );
  choose(file, new File([new Uint8Array(4)], "a.gif", { type: "image/gif" }));
  expect(screen.getByRole("alert").textContent).toBe(
    "Use a JPEG, PNG or WebP picture.",
  );
  expect(fetch).not.toHaveBeenCalled();
  expect(onSaved).not.toHaveBeenCalled();
});

test("the server's refusal is shown and nothing is reported saved", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json({ error: "That picture could not be read" }, { status: 422 }),
  );
  const { file, onSaved } = view(true);
  choose(file, new File([new Uint8Array(4)], "bad.png", { type: "image/png" }));
  await screen.findByText("That picture could not be read");
  expect(onSaved).not.toHaveBeenCalled();
});

test("removing sends a delete and reports it", async () => {
  const fetch = stubFetch();
  fetch.mockResolvedValueOnce(
    Response.json({
      id: "alice",
      username: "alice",
      admin: false,
      avatar_version: null,
    }),
  );
  const { onSaved } = view(true);
  fireEvent.click(screen.getByRole("button", { name: "Remove photo" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
  const [url, init] = fetch.mock.calls[0];
  expect(String(url)).toBe("/api/v1/account/avatar");
  expect(init?.method).toBe("DELETE");
  expect(screen.getByText(/Photo removed/)).toBeTruthy();
});
