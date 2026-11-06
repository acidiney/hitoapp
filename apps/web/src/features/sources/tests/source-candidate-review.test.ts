import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import RecordingDetailsPage from "../../recordings/pages/RecordingDetailsPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import {
  attachmentReview,
  attachmentSettings,
  savedRecording,
  sourceAttachmentPage,
} from "../fixtures/sourceAttachment";
import { automaticChoice } from "../fixtures/sourceChoice";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

const base = `/api/v1/recordings/${savedRecording.id}`;

function mount(
  items: Schema<"ProviderRecording">[],
  options: { choice?: Schema<"SourceChoice"> | null; review?: unknown } = {},
) {
  const attachments: unknown[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      if (path === `${base}/source-choice` && method === "GET")
        return options.choice
          ? Response.json(options.choice)
          : Response.json({ error: "Not found" }, { status: 404 });
      if (path === `${base}/download` && method === "GET")
        return Response.json({ error: "Not found" }, { status: 404 });
      if (path === "/api/v1/plugins") return Response.json(attachmentSettings);
      if (path === "/api/v1/plugins/video-connection/search")
        return Response.json({ items, next_cursor: null });
      if (path === `${base}/sources/review`)
        return Response.json(options.review ?? attachmentReview);
      if (path === `${base}/sources/attach`) {
        attachments.push(JSON.parse(String(init?.body)));
        return Response.json({ ...savedRecording, available: true });
      }
      return Response.json({ error: "Unexpected" }, { status: 500 });
    }),
  );
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: sourceAttachmentPage.user.id,
        children: createElement(RecordingDetailsPage, sourceAttachmentPage),
      }),
    ),
  );
  return { attachments };
}

async function search() {
  fireEvent.click(screen.getByRole("button", { name: "Find a source" }));
  await screen.findByRole("combobox", { name: "Music service" });
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "owned" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  return screen.findByRole("list", { name: "Source search results" });
}

const hit = (patch: Partial<Schema<"ProviderRecording">>) => ({
  id: "video/candidate",
  title: "Candidate title",
  artist: "Candidate artist",
  duration_ms: 121_000,
  ...patch,
});

test("each source candidate shows its artist, length, service and cover", async () => {
  mount([
    hit({}),
    hit({ id: "video/two", title: "Second", artist: "Other artist" }),
  ]);
  const rows = within(await search()).getAllByRole("listitem");
  expect(within(rows[0]).getByText("Candidate artist")).toBeTruthy();
  expect(within(rows[0]).getByText(/2:01/)).toBeTruthy();
  expect(within(rows[0]).getByText(/Video archive/)).toBeTruthy();
  expect(rows[0].querySelector(".artwork")).not.toBeNull();
  expect(within(rows[1]).getByText("Other artist")).toBeTruthy();
});

test("a candidate the service gave no artist or length for shows no Unknown placeholders", async () => {
  mount([hit({ artist: "Unknown artist", duration_ms: null })]);
  const [row] = within(await search()).getAllByRole("listitem");
  expect(within(row).queryByText(/Unknown/)).toBeNull();
  expect(within(row).queryByText(/Duration unknown/)).toBeNull();
  expect(within(row).getByText("Video archive")).toBeTruthy();
});

test("reviewing a candidate opens a modal beside the saved track with a length hint, and Cancel returns focus", async () => {
  mount([hit({})]);
  const list = await search();
  const open = within(list).getByRole("button", { name: "Review match" });
  open.focus();
  fireEvent.click(open);
  const dialog = await screen.findByRole("dialog", {
    name: "Review source match",
  });
  expect(dialog.tagName).toBe("DIALOG");
  expect(dialog.hasAttribute("open")).toBe(true);
  const saved = within(dialog).getByRole("group", { name: "Saved track" });
  const candidate = within(dialog).getByRole("group", {
    name: "Candidate source",
  });
  expect(within(saved).getByText(savedRecording.title)).toBeTruthy();
  expect(within(saved).getByText(savedRecording.artist)).toBeTruthy();
  expect(within(saved).getByText("2:00")).toBeTruthy();
  expect(saved.querySelector(".artwork")).not.toBeNull();
  expect(
    within(candidate).getByText(attachmentReview.candidate.title),
  ).toBeTruthy();
  expect(within(candidate).getByText("2:04")).toBeTruthy();
  expect(candidate.querySelector(".artwork")).not.toBeNull();
  expect(within(dialog).getByText(/4 seconds longer/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(
    within(list).getByRole("button", { name: "Review match" }),
  );
});

test("a much longer candidate is flagged and Use this source attaches it from the dialog", async () => {
  const { attachments } = mount([hit({})], {
    review: {
      ...attachmentReview,
      candidate: { ...attachmentReview.candidate, duration_ms: 245_000 },
    },
  });
  fireEvent.click(
    within(await search()).getByRole("button", { name: "Review match" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Review source match",
  });
  expect(within(dialog).getByText(/2:05 longer/)).toBeTruthy();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Use this source" }),
  );
  await screen.findByText("Source added to this track.");
  expect(attachments).toHaveLength(1);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the add-a-source card leads the source column for a track without a playable source", async () => {
  mount([hit({})]);
  const add = screen.getByRole("region", { name: "Add an audio source" });
  const side = screen.getByRole("complementary");
  expect(side.firstElementChild).toBe(add);
  expect(
    within(add).getByRole("button", { name: "Find a source" }).className,
  ).toContain("primary");
});

test("a track that already has sources keeps the card after the playback source choice", async () => {
  mount([hit({})], { choice: automaticChoice });
  const choice = await screen.findByRole("region", { name: "Playback source" });
  const add = screen.getByRole("region", { name: "Add an audio source" });
  expect(
    choice.compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  await waitFor(() => expect(screen.getByRole("complementary")).toBeTruthy());
});
