export function isOn(control: Element | null): boolean {
  return control?.getAttribute("aria-checked") === "true";
}
