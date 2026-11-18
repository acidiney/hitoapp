import { randomInt } from "node:crypto";

export function uniqueLoopbackHost() {
  return `127.${randomInt(1, 255)}.${randomInt(0, 256)}.${randomInt(1, 255)}`;
}
