const names = new Intl.DisplayNames(["en"], { type: "region" });

export function regionName(code: string) {
  try {
    return names.of(code.toUpperCase()) ?? code;
  } catch {
    return code;
  }
}
