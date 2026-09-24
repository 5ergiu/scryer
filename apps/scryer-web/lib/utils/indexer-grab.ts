import type { Release } from "@/lib/types/releases";
import type { TitleRecord } from "@/lib/types/titles";

export type GrabSubject = { name: string; year: number | null };
const normalized = (name: string) => name.toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();

export function grabSubjects(releases: Release[]): GrabSubject[] {
  const subjects = new Map<string, GrabSubject>();
  for (const release of releases) {
    const name = release.parsedRelease?.normalizedTitle.trim();
    if (!name) continue;
    const year = release.parsedRelease?.year ?? null;
    subjects.set(`${normalized(name)}:${year}`, { name, year });
  }
  return [...subjects.values()];
}

export function rankGrabSuggestions(titles: TitleRecord[], subjects: GrabSubject[]): TitleRecord[] {
  const rank = (title: TitleRecord) => Math.max(0, ...subjects.map((subject) => {
    const exact = normalized(title.name) === normalized(subject.name);
    return (exact ? 2 : 0) + (subject.year != null && title.year === subject.year ? 1 : 0);
  }));
  return [...new Map(titles.map((title) => [title.id, title])).values()].sort((a, b) =>
    rank(b) - rank(a) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) || a.id.localeCompare(b.id));
}

export type GrabClient = { id: string; name: string; category: string | null; mapped: boolean };
export type GrabRoutingRow = {
  rowKey: string;
  plain: GrabClient[];
  assigned: GrabClient[];
  plainError?: string;
  assignedError?: string;
};
export type GrabGroup = {
  id: string;
  rows: GrabRoutingRow[];
  clients: GrabClient[];
  clientId: string;
  category: string;
};

/** Group identical policies; mapped clients can never appear as alternatives. */
export function groupGrabRouting(rows: GrabRoutingRow[]): GrabGroup[] {
  const groups = new Map<string, GrabGroup>();
  for (const row of rows) {
    const clients = [...new Map([...row.plain, ...row.assigned].map((client) => [client.id, client])).values()];
    const id = JSON.stringify(clients.map((client) => [client.id, client.category, client.mapped]));
    const current = groups.get(id);
    if (current) current.rows.push(row);
    else groups.set(id, { id, rows: [row], clients, clientId: clients[0]?.id ?? "", category: clients[0]?.category ?? "" });
  }
  return [...groups.values()];
}

export function grabGroupAllows(group: GrabGroup, assign: boolean): boolean {
  return group.rows.every((row) => (assign ? row.assigned : row.plain).some((client) => client.id === group.clientId));
}

export function pendingGrabRows(releases: Release[], completed: ReadonlySet<string>, key: (release: Release) => string): Release[] {
  return releases.filter((release) => !completed.has(key(release)));
}
