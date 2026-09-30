import type { PlaybackAction, PlaybackView } from "../../contracts";

type PlaybackQueueProps = {
  snapshot: PlaybackView;
  run: (action: PlaybackAction) => Promise<void>;
  pending: Set<PlaybackAction["kind"]>;
};

export function PlaybackQueue({ snapshot, run, pending }: PlaybackQueueProps) {
  const queue = snapshot.queue;
  return (
    <section className="playback-queue" aria-labelledby="playback-queue-title">
      <div className="queue-heading">
        <h3 id="playback-queue-title">Queue</h3>
        <button
          type="button"
          className="secondary"
          disabled={queue.track_ids.length === 0 || pending.has("clear")}
          onClick={() => void run({ kind: "clear" })}
        >Clear queue</button>
      </div>
      {queue.track_ids.length === 0 ? <p>Queue is empty.</p> : (
        <ol className="queue-list">
          {queue.track_ids.map((trackId, index) => {
            const current = queue.current_index === index;
            const label = snapshot.track?.id === trackId
              ? snapshot.track.title
              : `Queued track ${index + 1}`;
            return (
              <li className={current ? "queue-current" : undefined} key={`${trackId}:${index}`}>
                <span>{label}{current ? " · Playing" : ""}</span>
                <div className="queue-actions">
                  <button
                    type="button"
                    className="secondary"
                    disabled={current || pending.has("playAt")}
                    aria-label={`Play queued track ${index + 1}`}
                    onClick={() => void run({ kind: "playAt", index })}
                  >Play</button>
                  <button
                    type="button"
                    className="secondary"
                    disabled={pending.has("remove")}
                    aria-label={`Remove queued track ${index + 1}`}
                    onClick={() => void run({ kind: "remove", index })}
                  >Remove</button>
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
