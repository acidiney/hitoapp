import { createElement, useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Dialog from "../ui/Dialog";

function Host() {
  const [open, setOpen] = useState(false);
  return createElement(
    "div",
    null,
    createElement("button", { onClick: () => setOpen(true) }, "Open"),
    open &&
      createElement(Dialog, {
        title: "Sample",
        onClose: () => setOpen(false),
        children: createElement("p", null, "Body"),
      }),
  );
}

test("closing a dialog with leave motion returns focus to the opener once the leaving copy is gone", () => {
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  render(createElement(Host));
  const opener = screen.getByRole("button", { name: "Open" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = document.querySelector("dialog")!;
  act(() => {
    dialog.dispatchEvent(new Event("close"));
  });
  act(() => {
    vi.advanceTimersByTime(200);
  });
  expect(document.activeElement).toBe(opener);
  expect(document.querySelector("dialog[data-leaving]")).toBeNull();
});
