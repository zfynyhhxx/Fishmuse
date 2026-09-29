import type { ChatToolActivity } from "../../state/chatStore";

const labels: Record<string, string> = {
  search_library: "Searched your library",
  get_library_item: "Opened a library item",
  get_recent_listens: "Reviewed recent listening",
  get_playback_state: "Checked playback",
  play_track: "Started playback",
  pause_playback: "Paused playback",
  resume_playback: "Resumed playback",
  seek_playback: "Changed playback position",
  skip_next: "Skipped to the next track",
};

export function ToolActivity({ activities }: { activities: ChatToolActivity[] }) {
  if (activities.length === 0) return null;
  return (
    <section className="tool-activity" aria-label="FishMuse activity">
      {activities.map((activity) => (
        <div className="tool-card" key={activity.id}>
          <span aria-hidden="true">{activity.status === "completed" ? "✓" : "…"}</span>
          <div><strong>{labels[activity.name] ?? "Used a music tool"}</strong><small>{activity.status}</small></div>
        </div>
      ))}
    </section>
  );
}
