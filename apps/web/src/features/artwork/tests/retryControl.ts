import { act, fireEvent, screen } from "@testing-library/react";

export async function retryControl(): Promise<HTMLElement> {
  const button = screen.queryByRole("button", { name: "Retry artwork" });
  if (button) return button;
  const item = screen.queryByRole("menuitem", { name: "Retry artwork" });
  if (item) return item;
  const opener = screen.getByRole("button", { name: "More options" });
  await act(async () => {
    fireEvent.click(opener);
  });
  return screen.getByRole("menuitem", { name: "Retry artwork" });
}

export function retryOffered(): boolean {
  return (
    screen.queryByRole("button", { name: "Retry artwork" }) !== null ||
    screen.queryByRole("button", { name: "More options" }) !== null
  );
}
