import { fireEvent, screen } from "@testing-library/react";

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

export async function continueFor(name: string) {
  await screen.findByRole("radio", {
    name: new RegExp(`^${escape(name)} \\(this credit\\)`, "i"),
  });
  return screen.getByRole("button", { name: "Continue" });
}

export async function chooseOption(name: RegExp) {
  fireEvent.click(await screen.findByRole("radio", { name }));
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}
