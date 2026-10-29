import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { PlaybackProvider } from "../../components/PlaybackProvider";
import PlayerDock from "../../components/PlayerDock";
import { nativeService } from "../../fixtures/native/service";

async function renderDock() {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "ready",
    desired_playing: true,
    playing: true,
  });
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock),
    }),
  );
  await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
  return service;
}

const speeds = () =>
  screen.getByRole("combobox", { name: "Playback speed" }) as HTMLSelectElement;
const disabled = () =>
  [...speeds().options].filter((o) => o.disabled).map((o) => o.value);
async function publish(
  service: ReturnType<typeof nativeService>,
  patch: object,
) {
  await act(async () => {
    Object.assign(service.state, patch);
    service.emit("state", service.state);
  });
}

test("a source that supports the chosen speed shows no notice and every option", async () => {
  const service = await renderDock();
  await publish(service, {
    speed: 1500,
    effective_speed: 1500,
    speed_support: { min: 500, max: 3000 },
  });
  expect(speeds().value).toBe("1.5");
  expect(disabled()).toEqual([]);
  expect(screen.queryByText(/plays at 1×/i)).toBeNull();
});

test("a unit-speed source keeps the chosen speed, plays at 1× and says so without blocking", async () => {
  const service = await renderDock();
  await publish(service, {
    speed: 1500,
    effective_speed: 1000,
    speed_support: { min: 1000, max: 1000 },
  });
  expect(speeds().value).toBe("1.5");
  expect(speeds().disabled).toBe(false);
  expect(disabled()).toEqual(["0.5", "0.75", "1.25", "2"]);
  const notice = await screen.findByText(
    "This source plays at 1×. Your 1.5× speed returns with a source that supports it.",
  );
  expect(notice.closest("[role=status]")).not.toBeNull();
  expect(screen.getByRole("button", { name: "Open queue" })).toHaveProperty(
    "disabled",
    false,
  );
});

test("the chosen speed is restored and the notice clears when a capable source returns", async () => {
  const service = await renderDock();
  await publish(service, {
    speed: 1500,
    effective_speed: 1000,
    speed_support: { min: 1000, max: 1000 },
  });
  await screen.findByText(/plays at 1×/i);
  await publish(service, {
    selection: 2,
    current: { id: "entry-1", recording_id: "recording-1" },
    speed_support: { min: 500, max: 3000 },
    effective_speed: 1500,
  });
  expect(speeds().value).toBe("1.5");
  expect(disabled()).toEqual([]);
  expect(screen.queryByText(/plays at 1×/i)).toBeNull();
});

test("a source with a narrower range disables only the speeds outside it", async () => {
  const service = await renderDock();
  await publish(service, {
    speed: 1000,
    effective_speed: 1000,
    speed_support: { min: 750, max: 1500 },
  });
  expect(disabled()).toEqual(["0.5", "2"]);
  expect(screen.queryByText(/plays at 1×/i)).toBeNull();
});

test("an unknown capability leaves every speed available", async () => {
  const service = await renderDock();
  await publish(service, { speed: 2000, speed_support: null });
  expect(disabled()).toEqual([]);
  expect(screen.queryByText(/plays at 1×/i)).toBeNull();
});
