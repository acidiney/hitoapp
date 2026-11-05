import { createElement } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import Avatar from "./Avatar";

test("without a photo the avatar shows initials and stays hidden from assistive technology", () => {
  const { container } = render(
    createElement(Avatar, { id: "alice", name: "Ana Souza" }),
  );
  expect(container.querySelector("img")).toBeNull();
  expect(container.textContent).toBe("AS");
  expect(container.firstElementChild?.getAttribute("aria-hidden")).toBe("true");
});

test("with a photo the avatar loads the versioned picture and names it after the account", () => {
  const { container } = render(
    createElement(Avatar, { id: "a/1", name: "Ana Souza", version: "abc123" }),
  );
  const image = screen.getByRole("img", { name: "Ana Souza" });
  expect(image.getAttribute("src")).toBe(
    "/api/v1/accounts/a%2F1/avatar?v=abc123",
  );
  expect(container.textContent).toBe("");
});

test("a picture that fails to load falls back to initials", () => {
  const { container } = render(
    createElement(Avatar, { id: "alice", name: "alice", version: "abc123" }),
  );
  fireEvent.error(screen.getByRole("img", { name: "alice" }));
  expect(container.querySelector("img")).toBeNull();
  expect(container.textContent).toBe("A");
});

test("a new version tries the picture again after a failure", () => {
  const { container, rerender } = render(
    createElement(Avatar, { id: "alice", name: "alice", version: "one" }),
  );
  fireEvent.error(screen.getByRole("img", { name: "alice" }));
  rerender(
    createElement(Avatar, { id: "alice", name: "alice", version: "two" }),
  );
  expect(container.querySelector("img")?.getAttribute("src")).toContain(
    "v=two",
  );
});
