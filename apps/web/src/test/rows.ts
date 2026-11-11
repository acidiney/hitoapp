export function entryText(row: HTMLElement) {
  return Array.from((row as HTMLTableRowElement).cells)
    .slice(1)
    .map((cell) => cell.textContent)
    .join("");
}
