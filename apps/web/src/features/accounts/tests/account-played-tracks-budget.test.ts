import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import AccountSettings from "../components/AccountSettings";
import { accountsApi } from "../api/accountsApi";
import { automaticCopiesApi } from "../../downloads/api/automaticCopiesApi";
import { NoticeProvider } from "../../../shared/ui/Notices";

vi.mock("../api/accountsApi", () => ({
  accountsApi: {
    list: vi.fn(),
    create: vi.fn(),
    setLibraryAccess: vi.fn(),
    remove: vi.fn(),
  },
}));
vi.mock("../../downloads/api/automaticCopiesApi", () => ({
  automaticCopiesApi: {
    status: vi.fn(),
    setKeep: vi.fn(),
    budget: vi.fn(),
    setBudget: vi.fn(),
  },
}));

const GIB = 1024 ** 3;
const admin = { id: "a", username: "admin", admin: true };
const listener = { id: "b", username: "listener", admin: false };

beforeEach(() => {
  vi.mocked(accountsApi.list).mockResolvedValue([admin, listener]);
  vi.mocked(automaticCopiesApi.budget).mockResolvedValue({
    budget_bytes: 5 * GIB,
  });
});

async function openBudget(name: string) {
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(AccountSettings, { account: "a", library: "lib" }),
    ),
  );
  await screen.findByText("listener");
  fireEvent.click(screen.getByRole("button", { name: `More for ${name}` }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Space for played tracks…" }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Space for played tracks",
  });
  const field = await within(dialog).findByLabelText("Space in GiB");
  await waitFor(() => expect((field as HTMLInputElement).value).toBe("5"));
  return { dialog, field: field as HTMLInputElement };
}

test("each account, including the administrator's own, offers its budget and only the budget", async () => {
  const { dialog } = await openBudget("admin");
  expect(automaticCopiesApi.budget).toHaveBeenCalledWith(
    "a",
    "a",
    expect.any(AbortSignal),
  );
  expect(within(dialog).getByText(/The default is 5 GiB/)).toBeTruthy();
  expect(within(dialog).queryByText(/used/i)).toBeNull();
  expect(automaticCopiesApi.status).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "More for admin" }));
  expect(
    screen.queryByRole("menuitem", { name: "Delete account…" }),
  ).toBeNull();
});

test("the budget must be a number from 0 to 1024 GiB", async () => {
  const { dialog, field } = await openBudget("listener");
  const save = within(dialog).getByRole("button", {
    name: "Save",
  }) as HTMLButtonElement;
  for (const value of ["1025", "-1", "", "lots"]) {
    fireEvent.change(field, { target: { value } });
    expect(save.disabled).toBe(true);
    expect(
      within(dialog).getByText("Enter a number from 0 to 1024."),
    ).toBeTruthy();
  }
  fireEvent.change(field, { target: { value: "0" } });
  expect(save.disabled).toBe(false);
  expect(
    within(dialog).queryByText("Enter a number from 0 to 1024."),
  ).toBeNull();
  fireEvent.change(field, { target: { value: "1024" } });
  expect(save.disabled).toBe(false);
  expect(automaticCopiesApi.setBudget).not.toHaveBeenCalled();
});

test("saving sends the budget in bytes and closes the dialog", async () => {
  vi.mocked(automaticCopiesApi.setBudget).mockResolvedValue({
    budget_bytes: 1.5 * GIB,
  });
  const { dialog, field } = await openBudget("listener");
  fireEvent.change(field, { target: { value: "1.5" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(automaticCopiesApi.setBudget).toHaveBeenCalledWith(
    "a",
    "b",
    1.5 * GIB,
    expect.any(AbortSignal),
  );
});

test("a refused save keeps the dialog open with the server's reason", async () => {
  vi.mocked(automaticCopiesApi.setBudget).mockRejectedValue(
    new Error("The budget is too large."),
  );
  const { dialog, field } = await openBudget("listener");
  fireEvent.change(field, { target: { value: "8" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  expect((await within(dialog).findByRole("alert")).textContent).toContain(
    "The budget is too large.",
  );
  expect(screen.getByRole("dialog")).toBeTruthy();
});
