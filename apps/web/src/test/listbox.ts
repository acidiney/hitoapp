import { fireEvent, within } from "@testing-library/react";

export function choose(control: HTMLElement, option: string | number) {
  fireEvent.click(control);
  const list = document.getElementById(
    control.getAttribute("aria-controls") ?? "",
  );
  if (!list) throw new Error("The combobox did not open a listbox");
  const options = within(list).getAllByRole("option");
  const target =
    typeof option === "number"
      ? options[option]
      : options.find((item) => item.textContent === option);
  if (!target) throw new Error(`No option ${String(option)}`);
  fireEvent.click(target);
}

export function optionsOf(control: HTMLElement) {
  if (control.getAttribute("aria-expanded") !== "true")
    fireEvent.click(control);
  const list = document.getElementById(
    control.getAttribute("aria-controls") ?? "",
  );
  if (!list) throw new Error("The combobox did not open a listbox");
  const labels = within(list)
    .getAllByRole("option")
    .map((item) => item.textContent ?? "");
  fireEvent.click(control);
  return labels;
}
