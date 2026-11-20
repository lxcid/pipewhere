import { answer, type ApiRequest } from "./api.ts";

/** A publication's place on the path to a remote post, per 00002 D3 and 00010 D4. */
export type PublicationState =
  | "awaiting_approval"
  | "scheduled"
  | "preparing"
  | "publishing"
  | "published"
  | "failed"
  | "outcome_unknown"
  | "cancelled";

export interface Publication {
  id: string;
  organization_id: string;
  channel_id: string;
  content_id: string;
  body: string;
  state: PublicationState;
  planned_at: string;
  tolerance_seconds: number;
  deadline: string;
  remote_id: string | null;
  remote_url: string | null;
}

/** One page of the publication list, newest first, per 00002 D14. */
export interface Page {
  publications: Publication[];
  next_cursor: string | null;
}

export interface PublicationAttempt {
  id: string;
  publication_id: string;
  result: "published" | "failed" | "outcome_unknown" | "cancelled" | null;
  reason: string | null;
  started_at: string;
  ended_at: string | null;
}

/** A new publication. Leaving out `planned_at` publishes now, by API's clock. */
export interface PublicationRequest {
  channel_id: string;
  body: string;
  planned_at?: string;
}

/** How API answered a create request, with `unconfirmed` as in `answer`. */
export type Sent =
  | { kind: "created"; publication: Publication }
  | { kind: "refused"; status: number; code: string }
  | { kind: "unconfirmed"; reason: string };

/** 16 random bytes in base64url, the 128 bits 00002 D5 asks for. */
export function newIdempotencyKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

/**
 * Sends new publications, keeping each one's `Idempotency-Key` until API says
 * whether it took effect. A submission whose outcome is unconfirmed is sent
 * again with its key and its first request, so API answers with the
 * publication it may already have created, per 00011 D1.
 */
export class Submitter {
  #send: (key: string, request: PublicationRequest) => Promise<Sent>;
  #pending: { key: string; request: PublicationRequest; sent: boolean } | null = null;
  #sending = false;

  constructor(send: (key: string, request: PublicationRequest) => Promise<Sent>) {
    this.#send = send;
  }

  /** The request whose outcome API has not confirmed, if any. */
  get unconfirmed(): PublicationRequest | null {
    return this.#sending ? null : (this.#pending?.request ?? null);
  }

  /** Answers null, and sends nothing, while an earlier submission is unanswered. */
  async submit(request: PublicationRequest): Promise<Sent | null> {
    if (this.#sending) return null;
    this.#pending ??= { key: newIdempotencyKey(), request, sent: false };
    const pending = this.#pending;
    this.#sending = true;
    try {
      const sent = await this.#send(pending.key, pending.request);
      // A refusal says only that this request did nothing. After an earlier
      // send went unanswered, that one may still have created the publication.
      if (sent.kind === "created" || (sent.kind === "refused" && !pending.sent)) {
        this.#pending = null;
      }
      return sent;
    } finally {
      pending.sent = true;
      this.#sending = false;
    }
  }

  /** Forgets an unconfirmed submission, so the next one is a new request. */
  discard() {
    if (!this.#sending) this.#pending = null;
  }
}

/** Creates publications in one organization, through `useApi` or `callApi`. */
export function sendCreate(
  api: <T>(path: string, request?: ApiRequest) => Promise<T>,
  organizationId: string,
) {
  return async (key: string, request: PublicationRequest): Promise<Sent> => {
    const sent = await answer(() =>
      api<Publication>(`/v1/orgs/${organizationId}/publications`, {
        method: "POST",
        body: request,
        headers: { "idempotency-key": key },
      }),
    );
    return sent.kind === "answered" ? { kind: "created", publication: sent.value } : sent;
  };
}

const LOCAL_TIME = /^(\d{4,})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?/;

/**
 * Reads a `datetime-local` value in the browser's time zone, per 00011 D2.
 * Answers the time in UTC, or why it cannot be scheduled.
 */
export function plannedAt(local: string, now: Date): { plannedAt: string } | { problem: string } {
  const parts = LOCAL_TIME.exec(local);
  if (!parts) return { problem: "Choose a date and time to publish." };
  const [year, month, day, hour, minute, second] = parts.slice(1).map((part) => Number(part ?? 0));
  if (year > 9999) return { problem: "Choose a year up to 9999." };
  // The Date constructor reads its fields in the local zone. A time the clock
  // skips moves forward, and one it repeats takes its first occurrence.
  const time = new Date(year, month - 1, day, hour, minute, second);
  if (Number.isNaN(time.getTime())) return { problem: "Choose a date and time to publish." };
  if (time <= now) return { problem: "That time has passed. Choose a later time, or publish now." };
  return { plannedAt: time.toISOString() };
}

const TIME = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit",
  timeZoneName: "short",
});

export function formatTime(iso: string): string {
  return TIME.format(new Date(iso));
}

/** Latest planned time first, per 00011 D3. Of two at one time, the newer. */
export function chronological(publications: Publication[]): Publication[] {
  return publications.toSorted(
    (a, b) =>
      Date.parse(b.planned_at) - Date.parse(a.planned_at) ||
      (a.id < b.id ? 1 : a.id > b.id ? -1 : 0),
  );
}

/**
 * Loads the list from its newest page down to the page that ended at `older`,
 * the cursor after the oldest page shown, per 00011 D4. Undefined loads only
 * the first page, and null loads every page. Answers the cursor after the
 * last page loaded.
 */
export async function loadThrough(
  fetchPage: (cursor: string | null) => Promise<Page>,
  older: string | null | undefined,
): Promise<{ publications: Publication[]; older: string | null }> {
  let page = await fetchPage(null);
  const publications = [...page.publications];
  // Ids sort in creation order, as API's cursor compares them.
  while (page.next_cursor && older !== undefined && (older === null || page.next_cursor > older)) {
    page = await fetchPage(page.next_cursor);
    publications.push(...page.publications);
  }
  return { publications, older: page.next_cursor };
}

/** A fresh page replaces what was loaded under the same ids. */
export function merged(loaded: Publication[], fresh: Publication[]): Publication[] {
  const ids = new Set(fresh.map((publication) => publication.id));
  return [...fresh, ...loaded.filter((publication) => !ids.has(publication.id))];
}

export interface StateView {
  label: string;
  tone: "waiting" | "working" | "published" | "failed" | "unknown" | "cancelled";
  detail?: string;
  cancellable: boolean;
  approvable: boolean;
}

/**
 * What a state means to the operator, and whether they may cancel or approve
 * it, per 00002 D3 and 00010 D4.
 */
export function describeState(publication: Publication, now: Date): StateView {
  return { ...describe(publication, now), approvable: publication.state === "awaiting_approval" };
}

function describe(publication: Publication, now: Date): Omit<StateView, "approvable"> {
  switch (publication.state) {
    case "awaiting_approval":
      return {
        label: "Awaiting approval",
        tone: "waiting",
        detail:
          Date.parse(publication.deadline) <= now.getTime()
            ? "Its time has passed, so approving it now fails without posting. Cancel it, and ask the agent for it again with a new time."
            : "An API key created this. It goes out at its time only after you approve it.",
        cancellable: true,
      };
    case "scheduled":
      return {
        label: "Scheduled",
        tone: "waiting",
        detail:
          Date.parse(publication.planned_at) <= now.getTime()
            ? `Waiting to start. If it has not started by ${formatTime(publication.deadline)}, it fails.`
            : undefined,
        cancellable: true,
      };
    case "preparing":
      return {
        label: "Preparing",
        tone: "working",
        detail: "Getting ready to send. It can still be cancelled.",
        cancellable: true,
      };
    case "publishing":
      return {
        label: "Publishing",
        tone: "working",
        detail: "Being sent to Mastodon. It can no longer be cancelled.",
        cancellable: false,
      };
    case "published":
      return { label: "Published", tone: "published", cancellable: false };
    case "failed":
      return { label: "Failed", tone: "failed", detail: "Not posted.", cancellable: false };
    case "outcome_unknown":
      return {
        label: "Outcome unknown",
        tone: "unknown",
        detail:
          "The post may be on Mastodon. Pipewhere will not send it again on its own, so check the account before posting this text again.",
        cancellable: false,
      };
    case "cancelled":
      return { label: "Cancelled", tone: "cancelled", detail: "Not posted.", cancellable: false };
  }
}

const REFUSED: Record<string, string> = {
  unauthorized: "Your session has ended. Sign in again.",
  not_found: "Pipewhere could not find it in this organization. Reload the page.",
  invalid_body:
    "Pipewhere refused the text or the time. Write some text, and choose a time no later than the year 9999.",
  channel_not_found: "That channel is no longer in this organization. Reload the page.",
  state_conflict:
    "It had already moved on, so that could no longer be done. The list shows where it is now.",
};

/** What to tell the operator about a request API refused or did not confirm. */
export function problemOf(sent: Exclude<Sent, { kind: "created" }>): string {
  if (sent.kind === "unconfirmed") return `Pipewhere did not confirm the request: ${sent.reason}`;
  return REFUSED[sent.code] ?? `Pipewhere refused the request (${sent.code}, ${sent.status}).`;
}
