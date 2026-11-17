"use client";

import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

import { answer } from "./api";
import { useApi } from "./api-client";
import type { Channel } from "./channels";
import {
  Submitter,
  chronological,
  describeState,
  formatTime,
  loadThrough,
  merged,
  plannedAt,
  problemOf,
  type Page,
  type Publication,
  type PublicationAttempt,
  type PublicationRequest,
  sendCreate,
} from "./publishing";

// Worker looks for due work every 5 seconds, so a publication sent now shows
// its outcome within one or two refreshes.
const REFRESH_MS = 10_000;

const WAITING: Publication["state"][] = ["scheduled", "preparing", "publishing"];

const CANNOT_PUBLISH: Record<Exclude<Channel["status"], "active">, string> = {
  disconnected: "Its channel is not connected.",
  access_revoked: "Mastodon no longer accepts Pipewhere's access to its channel.",
};

export function Publications({ organizationId }: { organizationId: string }) {
  const api = useApi();
  const [publications, setPublications] = useState<Publication[] | null>(null);
  const [channels, setChannels] = useState<Channel[] | null>(null);
  // The cursor after the oldest page shown: undefined until the first page
  // loads, and null once every page is shown. The ref lets a refresh read it
  // without being remade.
  const [older, setOlder] = useState<string | null | undefined>(undefined);
  const shownThrough = useRef<string | null | undefined>(undefined);
  const [problem, setProblem] = useState<string | null>(null);

  const fetchPage = useCallback(
    (cursor: string | null) =>
      api<Page>(
        `/v1/orgs/${organizationId}/publications${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ""}`,
      ),
    [api, organizationId],
  );

  const refresh = useCallback(async () => {
    const through = shownThrough.current;
    try {
      const [loaded, list] = await Promise.all([
        loadThrough(fetchPage, through),
        api<{ channels: Channel[] }>(`/v1/orgs/${organizationId}/channels`),
      ]);
      // "Show older" moved on while this refresh ran, and the next one covers it.
      if (shownThrough.current !== through) return;
      setChannels(list.channels);
      setPublications(loaded.publications);
      shownThrough.current = loaded.older;
      setOlder(loaded.older);
      setProblem(null);
    } catch (caught) {
      setProblem(
        `The list could not be loaded: ${caught instanceof Error ? caught.message : String(caught)}`,
      );
    }
  }, [api, fetchPage, organizationId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const waiting = publications?.some((publication) => WAITING.includes(publication.state));
  useEffect(() => {
    if (!waiting) return;
    const timer = setInterval(refresh, REFRESH_MS);
    return () => clearInterval(timer);
  }, [waiting, refresh]);

  async function loadOlder() {
    const cursor = shownThrough.current;
    if (!cursor) return;
    try {
      const page = await fetchPage(cursor);
      setPublications((loaded) => merged(loaded ?? [], page.publications));
      shownThrough.current = page.next_cursor;
      setOlder(page.next_cursor);
    } catch (caught) {
      setProblem(
        `Older publications could not be loaded: ${caught instanceof Error ? caught.message : String(caught)}`,
      );
    }
  }

  function show(publication: Publication) {
    setPublications((loaded) => merged(loaded ?? [], [publication]));
  }

  const handles = new Map(channels?.map((channel) => [channel.id, channel]));
  const active = channels?.filter((channel) => channel.status === "active") ?? [];

  return (
    <section className="card">
      <h2>Publications</h2>
      {/* Kept mounted when no channel can publish, so a draft and an unconfirmed submission survive a refresh. */}
      {channels && <Compose organizationId={organizationId} channels={active} onCreated={show} />}
      <div className="row spread">
        <h3>Chronological list</h3>
        <button className="ghost" onClick={refresh}>
          Refresh
        </button>
      </div>
      <p className="muted">Latest planned time first. Times are in your browser's time zone.</p>
      <ErrorLine problem={problem} />
      {publications?.length ? (
        <ul className="list">
          {chronological(publications).map((publication) => (
            <PublicationItem
              key={publication.id}
              organizationId={organizationId}
              publication={publication}
              channel={handles.get(publication.channel_id)}
              onChanged={(changed) => {
                if (changed) show(changed);
                refresh();
              }}
            />
          ))}
        </ul>
      ) : (
        publications && <p>No publications yet.</p>
      )}
      {older && (
        <button className="ghost older" onClick={loadOlder}>
          Show older
        </button>
      )}
    </section>
  );
}

function ErrorLine({ problem }: { problem: string | null }) {
  return problem ? (
    <p className="error" role="alert">
      {problem}
    </p>
  ) : null;
}

function Compose({
  organizationId,
  channels,
  onCreated,
}: {
  organizationId: string;
  channels: Channel[];
  onCreated: (publication: Publication) => void;
}) {
  const api = useApi();
  const [submitter] = useState(() => new Submitter(sendCreate(api, organizationId)));
  const [channelId, setChannelId] = useState(channels[0]?.id);
  // A refresh can find the chosen channel, or every channel, no longer active.
  const channel = channels.find((each) => each.id === channelId) ?? channels[0];
  const [body, setBody] = useState("");
  const [when, setWhen] = useState<"now" | "later">("now");
  const [local, setLocal] = useState("");
  const [sending, setSending] = useState(false);
  const [unconfirmed, setUnconfirmed] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const chosen = when === "later" && local ? plannedAt(local, new Date()) : null;

  async function send(request: PublicationRequest) {
    setSending(true);
    setProblem(null);
    setDone(null);
    const sent = await submitter.submit(request);
    // An earlier submission is still unanswered, and its own call settles the form.
    if (!sent) return;
    setSending(false);
    if (sent.kind === "created") {
      setUnconfirmed(null);
      setBody("");
      setLocal("");
      setDone(
        request.planned_at
          ? `Scheduled for ${formatTime(sent.publication.planned_at)}.`
          : "Sent to be published now. The list shows when it goes out.",
      );
      onCreated(sent.publication);
    } else if (sent.kind === "unconfirmed") {
      setUnconfirmed(sent.reason);
    } else {
      // A refused resend leaves the first send unconfirmed, and its notice stays.
      if (!submitter.unconfirmed) setUnconfirmed(null);
      setProblem(problemOf(sent));
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!channel) return;
    if (!body.trim()) {
      setProblem("Write some text to publish.");
      return;
    }
    const request: PublicationRequest = { channel_id: channel.id, body };
    if (when === "later") {
      const time = plannedAt(local, new Date());
      if ("problem" in time) {
        setProblem(time.problem);
        return;
      }
      request.planned_at = time.plannedAt;
    }
    send(request);
  }

  function resend() {
    const request = submitter.unconfirmed;
    if (request) send(request);
  }

  function discard() {
    submitter.discard();
    setUnconfirmed(null);
  }

  return (
    <form onSubmit={submit} className="compose">
      {!channel && (
        <p className="muted">Connect a Mastodon account above to write a publication.</p>
      )}
      <fieldset disabled={sending || unconfirmed !== null || !channel} className="stack">
        <label>
          Text
          <textarea
            value={body}
            required
            rows={4}
            onChange={(event) => setBody(event.target.value)}
          />
        </label>
        <label>
          Channel
          {!channel ? (
            <span className="value">None can publish</span>
          ) : channels.length > 1 ? (
            <select value={channel.id} onChange={(event) => setChannelId(event.target.value)}>
              {channels.map((channel) => (
                <option key={channel.id} value={channel.id}>
                  {channel.handle}
                </option>
              ))}
            </select>
          ) : (
            <span className="value">{channel.handle}</span>
          )}
        </label>
        <div className="choices" role="radiogroup" aria-label="When to publish">
          <label>
            <input
              type="radio"
              name="when"
              checked={when === "now"}
              onChange={() => setWhen("now")}
            />
            Publish now
          </label>
          <label>
            <input
              type="radio"
              name="when"
              checked={when === "later"}
              onChange={() => setWhen("later")}
            />
            Schedule for a time
          </label>
        </div>
        {when === "later" && (
          <label>
            Date and time, in {zone}
            <input
              type="datetime-local"
              value={local}
              required
              onChange={(event) => setLocal(event.target.value)}
            />
            {chosen && "plannedAt" in chosen && (
              <span>Goes out {formatTime(chosen.plannedAt)}.</span>
            )}
          </label>
        )}
        <div>
          <button type="submit">{when === "now" ? "Publish now" : "Schedule"}</button>
        </div>
      </fieldset>
      {unconfirmed !== null && (
        <div className="notice" role="alert">
          <p>
            Pipewhere did not confirm this publication, so it may have been created. Send it again
            to find out: Pipewhere answers with the first one instead of creating a second.
          </p>
          <p className="muted">{unconfirmed}</p>
          <div className="row">
            <button type="button" onClick={resend} disabled={sending}>
              Send again
            </button>
            <button type="button" className="ghost" onClick={discard} disabled={sending}>
              Discard
            </button>
          </div>
          <p className="muted">
            After you discard it, sending this text again can post it twice. Check the list first.
          </p>
        </div>
      )}
      <ErrorLine problem={problem} />
      {done && <p className="muted">{done}</p>}
    </form>
  );
}

function PublicationItem({
  organizationId,
  publication,
  channel,
  onChanged,
}: {
  organizationId: string;
  publication: Publication;
  channel: Channel | undefined;
  onChanged: (publication: Publication | null) => void;
}) {
  const api = useApi();
  const view = describeState(publication, new Date());
  const [confirming, setConfirming] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  async function cancel() {
    setCancelling(true);
    setProblem(null);
    const sent = await answer(() =>
      api<Publication>(`/v1/orgs/${organizationId}/publications/${publication.id}/cancel`, {
        method: "POST",
      }),
    );
    setCancelling(false);
    setConfirming(false);
    if (sent.kind === "answered") onChanged(sent.value);
    else {
      setProblem(problemOf(sent));
      onChanged(null);
    }
  }

  const blocked =
    publication.state === "scheduled" && channel && channel.status !== "active"
      ? `${CANNOT_PUBLISH[channel.status]} It fails at its planned time unless the account is connected again.`
      : null;

  return (
    <li className="publication">
      <div className="head">
        <span className={`state ${view.tone}`}>{view.label}</span>
        <span className="muted">
          {formatTime(publication.planned_at)}
          {channel?.handle && ` · ${channel.handle}`}
        </span>
      </div>
      <p className="body">{publication.body}</p>
      {view.detail && <p className="muted">{view.detail}</p>}
      {blocked && <p className="error">{blocked}</p>}
      {(publication.state === "failed" || publication.state === "outcome_unknown") && (
        <Reason organizationId={organizationId} publication={publication} />
      )}
      {publication.state === "published" &&
        publication.remote_url &&
        /^https?:\/\//i.test(publication.remote_url) && (
          <a href={publication.remote_url} target="_blank" rel="noreferrer">
            View the post
          </a>
        )}
      {view.cancellable &&
        (confirming ? (
          <div className="row">
            <span>Cancel this publication? It cannot be resumed.</span>
            <button className="ghost" onClick={cancel} disabled={cancelling}>
              Yes, cancel it
            </button>
            <button className="ghost" onClick={() => setConfirming(false)} disabled={cancelling}>
              Keep it
            </button>
          </div>
        ) : (
          <div>
            <button className="ghost" onClick={() => setConfirming(true)}>
              Cancel publication
            </button>
          </div>
        ))}
      <ErrorLine problem={problem} />
    </li>
  );
}

/** Why the newest attempt failed or left its outcome unknown, per 00002 D19 and D20. */
function Reason({
  organizationId,
  publication,
}: {
  organizationId: string;
  publication: Publication;
}) {
  const api = useApi();
  const [attempt, setAttempt] = useState<PublicationAttempt | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const load = useCallback(() => {
    setProblem(null);
    return api<{ attempts: PublicationAttempt[] }>(
      `/v1/orgs/${organizationId}/publications/${publication.id}/attempts`,
    )
      .then(({ attempts }) => setAttempt(attempts[0] ?? null))
      .catch((caught) =>
        setProblem(
          `The reason could not be loaded: ${caught instanceof Error ? caught.message : String(caught)}`,
        ),
      );
  }, [api, organizationId, publication.id]);

  // A new state can come with a new attempt.
  useEffect(() => {
    load();
  }, [load, publication.state]);

  if (problem) {
    return (
      <p className="error">
        {problem}{" "}
        <button className="link" onClick={load}>
          Try again
        </button>
      </p>
    );
  }
  if (!attempt?.reason) return null;
  return (
    <p className="reason">
      {attempt.reason}
      <span className="muted">
        Attempt ended {formatTime(attempt.ended_at ?? attempt.started_at)}.
      </span>
    </p>
  );
}
