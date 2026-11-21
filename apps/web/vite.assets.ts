export function assetFileNames({ names }: { names: string[] }): string {
  return names.some((name) => /\.(woff2|svg|png)$/.test(name))
    ? "assets/[name]-[hash][extname]"
    : "assets/app.[ext]";
}

type Emitted = { fileName: string };

export function oneStylesheet(bundle: Record<string, Emitted>): void {
  const sheets = Object.values(bundle)
    .map((file) => file.fileName)
    .filter((name) => name.endsWith(".css"));
  if (sheets.length !== 1 || sheets[0] !== "assets/app.css") {
    throw new Error(
      `The web build must produce one stylesheet, assets/app.css, and produced ${sheets.join(", ") || "none"}`,
    );
  }
}

export const stylesheetGuard = {
  name: "hito-one-stylesheet",
  generateBundle(_options: unknown, bundle: Record<string, Emitted>) {
    oneStylesheet(bundle);
  },
};
