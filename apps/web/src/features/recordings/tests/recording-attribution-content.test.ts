import { createElement } from "react";
import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import MetadataObservations from "../../metadata/components/MetadataObservations";
import {
  attributedRecordingPage,
  selectedRecordingAttribution,
  uncreditedRecordingObservation,
} from "../fixtures/recordingAttribution";
import type { RecordingAttribution } from "../../../shared/types";

function renderCredits(attribution: RecordingAttribution) {
  const logs = (["debug", "info", "log", "warn", "error"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const fetch = vi.fn(() => {
    throw new Error(
      "Rendering retained metadata credits cannot request content",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(MetadataObservations, {
      account: attributedRecordingPage.user.id,
      target: {
        kind: "recording",
        id: attributedRecordingPage.detail.recording_id,
      },
      initial: {
        items: [{ ...uncreditedRecordingObservation, attribution }],
        next_before: null,
      },
      selected: uncreditedRecordingObservation.id,
    }),
  );
  return {
    ...view,
    finish() {
      view.unmount();
      expect(fetch.mock.calls.length).toBe(0);
      expect(vi.mocked(console.warn).mock.calls.length).toBe(0);
      expect(vi.mocked(console.error).mock.calls.length).toBe(0);
      const output = JSON.stringify(logs.flatMap((log) => log.mock.calls));
      expect(output.length).toBeLessThanOrEqual(2048);
      for (const value of [
        ...attribution.creators,
        attribution.provider_label,
        attribution.content_url,
        attributedRecordingPage.detail.title.value,
        attributedRecordingPage.detail.artist.value,
      ])
        expect(output.includes(value)).toBe(false);
    },
  };
}

test("retained creator and provider markup remains literal text without creating remote content", () => {
  const creator =
    '<img src="https://images.example.invalid/pixel" onerror="owned()">';
  const provider = "<script>ownedProvider()</script>";
  const attribution: RecordingAttribution = {
    ...selectedRecordingAttribution,
    creators: [creator, "Owned literal collaborator", creator],
    provider_label: provider,
  };
  const view = renderCredits(attribution);
  try {
    expect(
      within(screen.getByRole("list", { name: "Creators" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(attribution.creators);
    expect(screen.getByRole("link", { name: provider }).textContent).toBe(
      provider,
    );
    expect(
      view.container.querySelectorAll("script,img,iframe,style,link"),
    ).toHaveLength(0);
    view.finish();
  } finally {
    view.unmount();
  }
});

test.each([
  {
    label: "canonical HTTPS",
    url: selectedRecordingAttribution.content_url,
    linked: true,
  },
  {
    label: "canonical HTTPS with port",
    url: "https://credits.example.invalid:8443/recording",
    linked: true,
  },
  {
    label: "canonical encoded path",
    url: "https://credits.example.invalid/recordings/part%20two",
    linked: true,
  },
  {
    label: "HTTP",
    url: "http://credits.example.invalid/recording",
    linked: false,
  },
  { label: "script scheme", url: "javascript:owned()", linked: false },
  { label: "data scheme", url: "data:text/html,owned", linked: false },
  { label: "relative path", url: "/recordings/owned", linked: false },
  {
    label: "protocol relative",
    url: "//credits.example.invalid/recording",
    linked: false,
  },
  {
    label: "username",
    url: "https://owned@credits.example.invalid/recording",
    linked: false,
  },
  {
    label: "password",
    url: "https://owned:secret@credits.example.invalid/recording",
    linked: false,
  },
  {
    label: "query",
    url: "https://credits.example.invalid/recording?owned=1",
    linked: false,
  },
  {
    label: "empty query",
    url: "https://credits.example.invalid/recording?",
    linked: false,
  },
  {
    label: "fragment",
    url: "https://credits.example.invalid/recording#owned",
    linked: false,
  },
  {
    label: "empty fragment",
    url: "https://credits.example.invalid/recording#",
    linked: false,
  },
  {
    label: "backslash",
    url: "https://credits.example.invalid\\recording",
    linked: false,
  },
  {
    label: "whitespace",
    url: " https://credits.example.invalid/recording",
    linked: false,
  },
  {
    label: "control character",
    url: "https://credits.example.invalid/rec\tording",
    linked: false,
  },
  {
    label: "noncanonical hostname",
    url: "https://CREDITS.example.invalid/recording",
    linked: false,
  },
  {
    label: "noncanonical default port",
    url: "https://credits.example.invalid:443/recording",
    linked: false,
  },
  {
    label: "noncanonical path",
    url: "https://credits.example.invalid/one/../recording",
    linked: false,
  },
  {
    label: "missing canonical slash",
    url: "https://credits.example.invalid",
    linked: false,
  },
])(
  "$label backlink keeps provider text and follows the canonical link contract",
  ({ url, linked }) => {
    const attribution: RecordingAttribution = {
      ...selectedRecordingAttribution,
      content_url: url,
    };
    const view = renderCredits(attribution);
    try {
      expect(
        screen.getByText(attribution.provider_label, { exact: true }),
      ).toBeTruthy();
      const link = screen.queryByRole("link", {
        name: attribution.provider_label,
      });
      if (linked) {
        expect(link?.getAttribute("href")).toBe(url);
        expect(link?.getAttribute("target")).toBe("_blank");
        expect(link?.getAttribute("rel")?.split(" ").sort()).toEqual([
          "noopener",
          "noreferrer",
        ]);
        expect(link?.getAttribute("referrerpolicy")).toBe("no-referrer");
      } else {
        expect(link).toBeNull();
        expect(screen.queryAllByRole("link")).toHaveLength(0);
      }
      view.finish();
    } finally {
      view.unmount();
    }
  },
);
