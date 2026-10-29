export function assetFileNames({ names }: { names: string[] }): string {
  return names.some((name) => name.endsWith(".woff2"))
    ? "assets/[name]-[hash][extname]"
    : "assets/app.[ext]";
}
