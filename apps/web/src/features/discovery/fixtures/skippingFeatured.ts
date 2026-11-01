import { vi, type Mock } from "vitest";

export function skippingFeatured(fetch: Mock) {
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) =>
    String(input) === "/api/v1/discover/featured"
      ? Promise.resolve(Response.json({ status: "unsupported" }))
      : (fetch(input, init) as Promise<Response>),
  );
}
