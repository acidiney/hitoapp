import { act } from "@testing-library/react";

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

export function heldResponse() {
  const { promise, resolve, reject } = deferred<Response>();
  return { response: promise, resolve, reject };
}

export async function flush() {
  await act(async () => {
    for (let step = 0; step < 8; step++) await Promise.resolve();
  });
}
