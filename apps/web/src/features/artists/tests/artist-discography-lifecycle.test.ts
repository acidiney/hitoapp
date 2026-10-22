import { createElement, type ComponentProps } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import type { components } from "../../../generated";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import {
  discographyTitle,
  firstDiscographyPage,
  hostDiscographyContinuation,
  lastDiscographyPage,
} from "../fixtures/artistDiscography";
import { discographyHttp } from "../fixtures/artistDiscographyHttp";
import { flush } from "../../../test/async";

type Schema = components["schemas"];
type Props = ComponentProps<typeof ArtistDiscography>;
const initial: Props = {
  account: artistReviewAccount,
  recording: artistReviewDetail.recording_id,
  association: artistAssociation,
  profile: reviewedArtistProfile.profile,
};
const lateTitle = "Private previous-owner release";
const freshTitle = "Current reviewed artist release";

function page(props: Props, title: string) {
  return {
    ...lastDiscographyPage,
    association_id: props.association.id,
    page: {
      ...lastDiscographyPage.page,
      artist_id: props.profile.artist.id,
      items: [
        {
          ...lastDiscographyPage.page.items[0],
          id: `owned:release/${title}`,
          title,
        },
      ],
    },
  } satisfies Schema["ArtistDiscographyAvailability"];
}

function path(props: Props) {
  return `/api/v1/recordings/${encodeURIComponent(props.recording)}/artist-associations/${encodeURIComponent(props.association.id)}/discography`;
}

async function browse() {
  fireEvent.click(screen.getByRole("button", { name: "Browse discography" }));
  return screen.findByRole("heading", { name: discographyTitle });
}

const changes: Array<[string, (props: Props) => Props]> = [
  [
    "account",
    (props) => ({ ...props, account: "70000000-0000-4000-8000-000000000001" }),
  ],
  [
    "recording",
    (props) => ({
      ...props,
      recording: "70000000-0000-4000-8000-000000000002",
    }),
  ],
  [
    "exact association",
    (props) => ({
      ...props,
      association: {
        ...props.association,
        id: "70000000-0000-4000-8000-000000000003",
      },
    }),
  ],
  [
    "provider",
    (props) => ({
      ...props,
      association: {
        ...props.association,
        provider: {
          kind: "http_plugin",
          installation_id: "70000000-0000-4000-8000-000000000004",
        },
      },
    }),
  ],
  [
    "accepted profile",
    (props) => ({
      ...props,
      profile: { ...props.profile, revision: "private-profile-revision:next" },
    }),
  ],
];

for (const mode of ["headers", "body"] as const) {
  test.each(changes)(
    `changing %s retires previous-owner ${mode} before an explicit new browse`,
    async (_label, change) => {
      const errors = vi.spyOn(console, "error");
      const http = discographyHttp();
      http.reply(firstDiscographyPage);
      const held = http.hold(
        mode,
        mode === "headers"
          ? { error: "Private previous-owner failure" }
          : page(initial, lateTitle),
        mode === "headers" ? 409 : 200,
      );
      const view = render(createElement(ArtistDiscography, initial));
      try {
        await browse();
        fireEvent.click(
          screen.getByRole("button", { name: "Load more releases" }),
        );
        await act(async () => {
          await held.entered;
        });
        if (mode === "body") expect(held.response.bodyUsed).toBe(true);
        const changed = change(initial);
        view.rerender(createElement(ArtistDiscography, changed));
        expect(held.signal()?.aborted).toBe(true);
        expect(held.aborts()).toBe(1);
        expect(
          screen.queryByRole("region", { name: "Discography" }),
        ).toBeNull();
        expect(
          screen.queryByRole("heading", { name: discographyTitle }),
        ).toBeNull();
        expect(http.fetch).toHaveBeenCalledTimes(2);
        const current: Props = {
          ...changed,
          association: {
            ...changed.association,
            id:
              changed.account !== initial.account ||
              changed.recording !== initial.recording ||
              JSON.stringify(changed.association.provider) !==
                JSON.stringify(initial.association.provider)
                ? "80000000-0000-4000-8000-000000000001"
                : changed.association.id,
            account_id: changed.account,
            recording_id: changed.recording,
          },
        };
        view.rerender(createElement(ArtistDiscography, current));
        http.reply(page(current, freshTitle));
        fireEvent.click(
          screen.getByRole("button", { name: "Browse discography" }),
        );
        await screen.findByRole("heading", { name: freshTitle });
        await act(async () => {
          held.release();
        });
        await flush();
        expect(screen.getByRole("heading", { name: freshTitle })).toBeTruthy();
        expect(screen.queryByRole("heading", { name: lateTitle })).toBeNull();
        expect(
          screen.queryByText("Private previous-owner failure", { exact: true }),
        ).toBeNull();
        expect(screen.queryByRole("alert")).toBeNull();
        expect(
          screen.queryByRole("button", { name: "Restart discography" }),
        ).toBeNull();
        expect(http.fetch).toHaveBeenCalledTimes(3);
        expect(http.fetch.mock.calls[2][0]).toBe(path(current));
        expect(JSON.parse(http.fetch.mock.calls[2][1]?.body as string)).toEqual(
          {
            limit: 20,
            continuation: null,
          } satisfies Schema["ReadArtistDiscography"],
        );
        expect(
          new Headers(http.fetch.mock.calls[2][1]?.headers).get(
            "x-hito-account",
          ),
        ).toBe(current.account);
        expect(errors).not.toHaveBeenCalled();
      } finally {
        await act(async () => {
          view.unmount();
          http.close();
        });
      }
    },
  );

  test(`unmount aborts owned discography ${mode} and ignores its late completion`, async () => {
    const errors = vi.spyOn(console, "error");
    const http = discographyHttp();
    const held = http.hold(
      mode,
      mode === "headers"
        ? { error: "Private unmounted failure" }
        : page(initial, lateTitle),
      mode === "headers" ? 404 : 200,
    );
    const view = render(createElement(ArtistDiscography, initial));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Browse discography" }),
      );
      await act(async () => {
        await held.entered;
      });
      view.unmount();
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      await act(async () => {
        held.release();
      });
      await flush();
      expect(screen.queryByRole("region", { name: "Discography" })).toBeNull();
      expect(
        screen.queryByText("Private unmounted failure", { exact: true }),
      ).toBeNull();
      expect(http.fetch).toHaveBeenCalledTimes(1);
      expect(errors).not.toHaveBeenCalled();
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  });
}

test("a temporary later-page failure keeps known releases and retries the same host continuation explicitly", async () => {
  const http = discographyHttp();
  http.reply(firstDiscographyPage);
  http.reply({ error: "The owned catalog is temporarily unavailable." }, 503);
  http.reply(lastDiscographyPage);
  const view = render(createElement(ArtistDiscography, initial));
  try {
    await browse();
    fireEvent.click(screen.getByRole("button", { name: "Load more releases" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The owned catalog is temporarily unavailable.",
    );
    expect(
      screen.getByRole("heading", { name: discographyTitle }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Restart discography" }),
    ).toBeNull();
    view.rerender(createElement(ArtistDiscography, initial));
    await flush();
    expect(http.fetch).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("heading", { name: discographyTitle }),
      ).toHaveLength(2),
    );
    expect(http.fetch).toHaveBeenCalledTimes(3);
    for (const [, options] of http.fetch.mock.calls.slice(1))
      expect(JSON.parse(options?.body as string)).toEqual({
        limit: 20,
        continuation: hostDiscographyContinuation,
      } satisfies Schema["ReadArtistDiscography"]);
    expect(screen.queryByRole("alert")).toBeNull();
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});

test.each([403, 404])(
  "a complete %s response cannot bypass discography authority or invent a restart",
  async (status) => {
    const http = discographyHttp();
    http.reply(firstDiscographyPage);
    const error =
      status === 403
        ? "You do not have access to this resource"
        : "Resource not found";
    http.reply({ error }, status);
    http.reply({ error }, status);
    const view = render(createElement(ArtistDiscography, initial));
    try {
      await browse();
      fireEvent.click(
        screen.getByRole("button", { name: "Load more releases" }),
      );
      expect((await screen.findByRole("alert")).textContent).toBe(error);
      expect(
        screen.queryByRole("button", { name: "Restart discography" }),
      ).toBeNull();
      await flush();
      expect(http.fetch).toHaveBeenCalledTimes(2);
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      expect((await screen.findByRole("alert")).textContent).toBe(error);
      expect(http.fetch).toHaveBeenCalledTimes(3);
      for (const [requested, options] of http.fetch.mock.calls) {
        expect(requested).toBe(path(initial));
        expect(new Headers(options?.headers).get("x-hito-account")).toBe(
          initial.account,
        );
        expect(options?.credentials).toBe("same-origin");
      }
      for (const [, options] of http.fetch.mock.calls.slice(1))
        expect(JSON.parse(options?.body as string)).toEqual({
          limit: 20,
          continuation: hostDiscographyContinuation,
        } satisfies Schema["ReadArtistDiscography"]);
      expect(
        screen.queryByRole("button", {
          name: /play|download|import|add to library/i,
        }),
      ).toBeNull();
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  },
);

test.each([
  ["disabled", /disabled|settings/i],
  ["unsupported", /does not support|unsupported/i],
  ["unavailable", /unavailable/i],
  ["cancelled", /cancelled/i],
] as const)(
  "a %s catalog result stays truthful and read-only",
  async (status, message) => {
    const http = discographyHttp();
    http.reply({ status } satisfies Schema["ArtistDiscographyAvailability"]);
    const view = render(createElement(ArtistDiscography, initial));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Browse discography" }),
      );
      expect((await screen.findByRole("alert")).textContent).toMatch(message);
      const region = screen.getByRole("region", { name: "Discography" });
      expect(within(region).queryAllByRole("listitem")).toHaveLength(0);
      expect(
        within(region).queryByRole("button", { name: "Load more releases" }),
      ).toBeNull();
      expect(
        within(region).queryByRole("button", { name: "Restart discography" }),
      ).toBeNull();
      expect(
        within(region).queryByRole("button", {
          name: /play|download|import|add to library/i,
        }),
      ).toBeNull();
      await flush();
      expect(http.fetch).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  },
);
