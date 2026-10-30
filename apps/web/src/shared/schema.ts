import type { components } from "../generated";

export type Schema<Name extends keyof components["schemas"]> =
  components["schemas"][Name];
