export function assetFileNames({ names }: { names: string[] }): string {
  return names.some((name) => /\.(woff2|svg|png)$/.test(name))
    ? "assets/[name]-[hash][extname]"
    : "assets/app.[ext]";
}
