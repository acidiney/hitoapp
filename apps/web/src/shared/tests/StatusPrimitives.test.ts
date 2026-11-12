import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import AvailabilityLabel from "../ui/AvailabilityLabel";
import HealthDot from "../ui/HealthDot";
import StatusDot from "../ui/StatusDot";
import SectionWarning from "../ui/SectionWarning";
import Button from "../ui/Button";

test("availability shows a ring and the words for each state", () => {
  const { container } = render(
    createElement(
      "div",
      null,
      createElement(AvailabilityLabel, { state: "library" }),
      createElement(AvailabilityLabel, { state: "available" }),
      createElement(AvailabilityLabel, { state: "catalog" }),
    ),
  );
  expect(screen.getByText("In library")).toBeTruthy();
  expect(screen.getByText("Available")).toBeTruthy();
  expect(screen.getByText("Catalog only")).toBeTruthy();
  expect(
    [...container.querySelectorAll("[data-ring]")].map((ring) =>
      ring.getAttribute("data-ring"),
    ),
  ).toEqual(["library", "available", "catalog"]);
  for (const ring of container.querySelectorAll("[data-ring]"))
    expect(ring.getAttribute("aria-hidden")).toBe("true");
});

test("a health dot says its state in words, not colour alone", () => {
  render(createElement(HealthDot, { tone: "warning", label: "Slow" }));
  expect(screen.getByText("Slow")).toBeTruthy();
});

test("a warning is announced politely and an error assertively", () => {
  render(
    createElement(
      "div",
      null,
      createElement(
        SectionWarning,
        { tone: "warning", title: "Spotify is slow." },
        "Your library still plays.",
      ),
      createElement(
        SectionWarning,
        { tone: "error", title: "Folder missing." },
        "Reconnect the drive.",
      ),
    ),
  );
  expect(screen.getByRole("status").textContent).toContain("Spotify is slow.");
  expect(screen.getByRole("alert").textContent).toContain("Folder missing.");
});

test("buttons default to type button and expose the active state", () => {
  render(
    createElement(
      "div",
      null,
      createElement(Button, { variant: "primary" }, "Play"),
      createElement(Button, { variant: "active", pressed: true }, "Following"),
      createElement(Button, { variant: "destructive" }, "Remove"),
    ),
  );
  expect(
    screen.getByRole("button", { name: "Play" }).getAttribute("type"),
  ).toBe("button");
  expect(
    screen
      .getByRole("button", { name: "Following" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  expect(screen.getByRole("button", { name: "Remove" }).className).toContain(
    "destructive",
  );
});

test("a source status always puts a word beside its dot", () => {
  const { container } = render(
    createElement(
      "div",
      null,
      createElement(StatusDot, { status: "ready", detail: "Local" }),
      createElement(StatusDot, { status: "checking" }),
      createElement(StatusDot, { status: "none" }),
      createElement(StatusDot, { status: "failed" }),
    ),
  );
  expect(
    [...container.querySelectorAll(".source-status")].map(
      (status) => status.textContent,
    ),
  ).toEqual(["Ready · Local", "Checking…", "No source", "Couldn't check"]);
  expect(
    [...container.querySelectorAll(".source-status")].map((status) =>
      status.getAttribute("data-status"),
    ),
  ).toEqual(["ready", "checking", "none", "failed"]);
  for (const dot of container.querySelectorAll(".source-status-dot"))
    expect(dot.getAttribute("aria-hidden")).toBe("true");
});
