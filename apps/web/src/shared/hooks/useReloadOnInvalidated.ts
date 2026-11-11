import { useEffect } from "react";
import { router } from "@inertiajs/react";
import { onInvalidated, type InvalidationScope } from "../invalidation";

export function useReloadOnInvalidated(
  scope: InvalidationScope,
  only: string[],
) {
  const key = only.join("\u0000");
  useEffect(
    () =>
      onInvalidated(scope, () => router.reload({ only: key.split("\u0000") })),
    [scope, key],
  );
}
