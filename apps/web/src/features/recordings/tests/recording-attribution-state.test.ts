import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import {
  attributedRecordingPage,
  historicalRecordingAttribution,
  historicalRecordingObservation,
  historicalRecordingPage,
  selectedRecordingAttribution,
  selectedRecordingObservation,
  uncreditedRecordingObservation,
} from "../fixtures/recordingAttribution";
import { recordingAttributionView } from "../fixtures/recordingAttributionView";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test.each(["absent", "null"] as const)(
  "%s recording credits leave no empty selected section or creator list",
  async (presence) => {
    const observation =
      presence === "absent"
        ? uncreditedRecordingObservation
        : { ...uncreditedRecordingObservation, attribution: null };
    const page: Schema<"RecordingDetailsPageProps"> = {
      ...attributedRecordingPage,
      detail: {
        ...attributedRecordingPage.detail,
        selected_observation: observation,
        observations: { items: [observation], next_before: null },
      },
    };
    const view = recordingAttributionView([page]);
    try {
      await view.settled(1);
      expect(
        screen.queryByRole("region", { name: "Selected metadata credits" }),
      ).toBeNull();
      expect(screen.queryByRole("list", { name: "Creators" })).toBeNull();
      expect(screen.queryByText("Creators", { exact: true })).toBeNull();
      expect(
        screen.getByRole("heading", {
          name: page.detail.title.value,
          level: 1,
        }),
      ).toBeTruthy();
      expect(router.reload).not.toHaveBeenCalled();
      expect(router.visit).not.toHaveBeenCalled();
      view.finish();
    } finally {
      view.unmount();
    }
  },
);

test("metadata history keeps each observation's retained credits while personal drafts stay independent", async () => {
  const before = structuredClone(historicalRecordingPage);
  const view = recordingAttributionView([historicalRecordingPage]);
  try {
    await view.settled(1);
    const history = within(
      screen.getByRole("region", { name: "Source observations" }),
    );
    const observations = history.getAllByRole("article");
    expect(observations).toHaveLength(2);
    for (const [index, credit] of [
      selectedRecordingAttribution,
      historicalRecordingAttribution,
    ].entries()) {
      const observation = within(observations[index]);
      expect(
        within(observation.getByRole("list", { name: "Creators" }))
          .getAllByRole("listitem")
          .map((creator) => creator.textContent),
      ).toEqual(credit.creators);
      expect(
        observation
          .getByRole("link", { name: credit.provider_label })
          .getAttribute("href"),
      ).toBe(credit.content_url);
    }
    expect(within(observations[0]).getByText("Current metadata")).toBeTruthy();
    expect(within(observations[1]).queryByText("Current metadata")).toBeNull();
    const title = screen.getByRole<HTMLInputElement>("textbox", {
      name: "Track title",
    });
    const artist = screen.getByRole<HTMLInputElement>("textbox", {
      name: "Track artist",
    });
    const drafts = [
      "Unsent owned personal title",
      "Unsent owned personal artist",
    ];
    fireEvent.change(title, { target: { value: drafts[0] } });
    fireEvent.change(artist, { target: { value: drafts[1] } });
    expect(title.value).toBe(drafts[0]);
    expect(artist.value).toBe(drafts[1]);
    expect(history.getAllByRole("article")).toEqual(observations);
    expect(
      screen.getByRole("heading", {
        name: historicalRecordingPage.detail.title.value,
        level: 1,
      }),
    ).toBeTruthy();
    expect(historicalRecordingPage).toEqual(before);
    await view.settled(1);
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    view.finish(drafts);
  } finally {
    view.unmount();
  }
});

test.each(["account", "recording"] as const)(
  "replacing the %s removes old credits and ignores a late history response",
  async (owner) => {
    const initial: Schema<"RecordingDetailsPageProps"> = {
      ...attributedRecordingPage,
      detail: {
        ...attributedRecordingPage.detail,
        observations: { items: [selectedRecordingObservation], next_before: 1 },
      },
    };
    const replacementObservation = {
      ...selectedRecordingObservation,
      id: "331ff549-42f6-4294-92d4-3079e25b5851",
      attribution: {
        creators: ["Owned replacement creator"],
        provider_label: "Owned replacement catalogue",
        content_url: "https://credits.example.invalid/recordings/replacement",
      },
    };
    const replacement: Schema<"RecordingDetailsPageProps"> = {
      ...initial,
      user:
        owner === "account"
          ? { ...initial.user, id: "owned-replacement-account" }
          : initial.user,
      detail: {
        ...initial.detail,
        recording_id:
          owner === "recording"
            ? "857b5d7e-05c1-401e-8cb1-af08c74bb557"
            : initial.detail.recording_id,
        title: {
          ...initial.detail.title,
          value: "Owned replacement personal title",
          personal: "Owned replacement personal title",
        },
        artist: {
          ...initial.detail.artist,
          value: "Owned replacement personal artist",
          personal: "Owned replacement personal artist",
        },
        selected_observation: replacementObservation,
        observations: { items: [replacementObservation], next_before: null },
      },
    };
    const late = Response.json({
      ...initial.detail,
      observations: {
        items: [historicalRecordingObservation],
        next_before: null,
      },
    });
    let release!: () => void;
    const response = new Promise<Response>((resolve) => {
      release = () => resolve(late);
    });
    const path = `/api/v1/recordings/${initial.detail.recording_id}/metadata?before=1`;
    const view = recordingAttributionView([initial, replacement], {
      account: initial.user.id,
      path,
      response,
    });
    try {
      await view.settled(1);
      fireEvent.click(
        screen.getByRole("button", { name: "Older observations" }),
      );
      await view.settled(1, true);
      const request = view.fetch.mock.calls.find(
        ([requested]) => requested === path,
      );
      expect(request?.[1]?.signal?.aborted).toBe(false);
      view.replace(replacement);
      await view.settled(2, true);
      expect(request?.[1]?.signal?.aborted).toBe(true);
      await act(async () => release());
      await waitFor(() => expect(late.bodyUsed).toBe(true));
      const credits = within(
        screen.getByRole("region", { name: "Selected metadata credits" }),
      );
      expect(
        credits
          .getByRole("link", {
            name: replacementObservation.attribution.provider_label,
          })
          .getAttribute("href"),
      ).toBe(replacementObservation.attribution.content_url);
      expect(
        screen.queryByText(selectedRecordingAttribution.creators[0], {
          exact: true,
        }),
      ).toBeNull();
      expect(
        screen.queryByRole("link", {
          name: selectedRecordingAttribution.provider_label,
        }),
      ).toBeNull();
      expect(
        screen.queryByRole("link", {
          name: historicalRecordingAttribution.provider_label,
        }),
      ).toBeNull();
      expect(
        screen.getByRole("heading", {
          name: replacement.detail.title.value,
          level: 1,
        }),
      ).toBeTruthy();
      expect(router.reload).not.toHaveBeenCalled();
      expect(router.visit).not.toHaveBeenCalled();
      await view.settled(2, true);
      view.finish([
        historicalRecordingObservation.title,
        historicalRecordingObservation.artist,
        ...historicalRecordingAttribution.creators,
        historicalRecordingAttribution.provider_label,
        historicalRecordingAttribution.content_url,
      ]);
    } finally {
      view.unmount();
      await act(async () => release());
    }
  },
);
