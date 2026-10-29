import { createElement, useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Dialog from "./Dialog";

function Host({ onClose }: { onClose: () => void }) {
  const [open, setOpen] = useState(false);
  return createElement(
    "div",
    null,
    createElement("button", { onClick: () => setOpen(true) }, "Open"),
    open &&
      createElement(
        Dialog,
        {
          title: "Example",
          onClose: () => {
            onClose();
            setOpen(false);
          },
          children: null,
        },
        createElement("button", null, "Inside"),
      ),
  );
}

test("a native close event asks the owner to close", () => {
  const onClose = vi.fn();
  render(createElement(Host, { onClose }));
  fireEvent.click(screen.getByRole("button", { name: "Open" }));
  const dialog = screen.getByRole("dialog", { name: "Example" });
  fireEvent(dialog, new Event("close"));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("closing returns focus to the element that opened the dialog", () => {
  render(createElement(Host, { onClose: () => {} }));
  const trigger = screen.getByRole("button", { name: "Open" });
  trigger.focus();
  fireEvent.click(trigger);
  fireEvent(
    screen.getByRole("dialog"),
    new Event("cancel", { cancelable: true }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(trigger);
});
