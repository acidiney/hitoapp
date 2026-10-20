import { createHash } from "node:crypto";
import { readdir, readFile, lstat } from "node:fs/promises";
import { join } from "node:path";

const files = [];
let bytes = 0;
async function visit(path) {
  const metadata = await lstat(path);
  if (metadata.isSymbolicLink())
    throw new Error("Owned browser module symlink changed");
  if (metadata.isDirectory()) {
    for (const child of (await readdir(path)).sort())
      await visit(join(path, child));
  } else {
    if (
      !metadata.isFile() ||
      files.length >= 10_000 ||
      (bytes += metadata.size) > 128 * 1024 * 1024
    )
      throw new Error("Owned browser modules exceeded their bound");
    const contents = await readFile(path);
    files.push([
      path,
      metadata.size,
      createHash("sha256").update(contents).digest("hex"),
    ]);
  }
}
for (const name of ["@playwright/test", "playwright", "playwright-core"]) {
  const root = `node_modules/${name}`;
  const metadata = JSON.parse(await readFile(`${root}/package.json`, "utf8"));
  if (metadata.version !== "1.56.1")
    throw new Error("Owned browser modules changed version");
  await visit(root);
}
console.info(
  JSON.stringify({
    version: "1.56.1",
    files: files.length,
    bytes,
    sha256: createHash("sha256").update(JSON.stringify(files)).digest("hex"),
  }),
);
