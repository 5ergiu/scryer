import type { MediaRequestRecord } from "@/lib/types/titles";

export type MediaRequestStatusFilter = "all" | MediaRequestRecord["status"];

// The requests page loads every status at once and filters here, so each tab's
// count comes from the same list the other tabs are drawn from. Counting a list
// the server had already narrowed to one status left every other tab at zero.

export function requestsWithStatus(
  requests: readonly MediaRequestRecord[],
  status: MediaRequestStatusFilter,
): MediaRequestRecord[] {
  if (status === "all") {
    return [...requests];
  }
  return requests.filter((request) => request.status === status);
}

export function requestCountByStatus(
  requests: readonly MediaRequestRecord[],
  status: MediaRequestStatusFilter,
): number {
  return requestsWithStatus(requests, status).length;
}

export function requestCountByFacet(
  requests: readonly MediaRequestRecord[],
  facet: MediaRequestRecord["facet"],
): number {
  return requests.filter((request) => request.facet === facet).length;
}
