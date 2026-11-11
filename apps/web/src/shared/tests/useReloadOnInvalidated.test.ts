import { renderHook, act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import { useReloadOnInvalidated } from "../hooks/useReloadOnInvalidated";
import { invalidate } from "../invalidation";

vi.mock("@inertiajs/react", () => ({ router: { reload: vi.fn() } }));

test("an announced change reloads only the named page props and stops after unmount", () => {
  const view = renderHook(() => useReloadOnInvalidated("playlists", ["playlists"]));
  act(() => invalidate("playlists"));
  expect(router.reload).toHaveBeenCalledWith({ only: ["playlists"] });
  act(() => invalidate("recording", "r1"));
  expect(router.reload).toHaveBeenCalledTimes(1);
  view.unmount();
  act(() => invalidate("playlists"));
  expect(router.reload).toHaveBeenCalledTimes(1);
});
