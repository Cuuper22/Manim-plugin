import type { JobSummary, Progress } from "../api/types.ts";
import type { Workspace } from "../store/reducer.ts";

const LISTED_JOBS = 20;

interface OverviewProps {
  workspace: Workspace;
  jobs: readonly JobSummary[];
  reconnecting: boolean;
}

/** A plain read-out of the live workspace until the full workbench lands. */
export function Overview({ workspace, jobs, reconnecting }: OverviewProps) {
  const { project, scenes, latest, findings, scene_index: index } = workspace;
  return (
    <main className="overview">
      <header>
        <h1>{project.name}</h1>
        <span className="muted">{project.root}</span>
        <span className="muted">engine {workspace.engine.version}</span>
      </header>
      {reconnecting ? <p className="banner" role="status">Reconnecting to the engine; what you see may be out of date.</p> : null}

      <section aria-labelledby="scenes-heading">
        <h2 id="scenes-heading">Scenes {index.state === "indexing" ? <span className="muted">indexing…</span> : null}</h2>
        <ul>
          {scenes.map((scene) => {
            const video = latest[scene.id]?.video;
            return (
              <li key={scene.id}>
                <span>{scene.class_name}</span>
                <span className="muted">{scene.file}:{scene.span.start}</span>
                {scene.parse_failed ? <span className="severity-error">does not parse</span> : null}
                {video ? <a href={video.artifact.url}>{video.outdated ? "latest render (outdated)" : "latest render"}</a> : null}
              </li>
            );
          })}
        </ul>
      </section>

      <section aria-labelledby="jobs-heading">
        <h2 id="jobs-heading">Jobs</h2>
        <ul>
          {jobs.slice(0, LISTED_JOBS).map((job) => (
            <li key={job.id}>
              <span>{job.operation}</span>
              <span className="muted">{job.scene_id ?? ""}</span>
              <span className={`status-${job.status}`}>{job.cached ? `${job.status} (cached)` : job.status}</span>
              {job.progress ? <span className="muted">{progressText(job.progress)}</span> : null}
              {job.error ? <span className="severity-error">{job.error.message}</span> : null}
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="findings-heading">
        <h2 id="findings-heading">Findings</h2>
        <ul>
          {findings.map((finding) => (
            <li key={finding.id}>
              <span className={`severity-${finding.severity}`}>{finding.severity}</span>
              <span>{finding.message}</span>
              {finding.location ? (
                <span className="muted">
                  {finding.location.file}:{finding.location.line}
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}

function progressText(progress: Progress): string {
  const count = progress.total === null ? `${progress.current}` : `${progress.current}/${progress.total}`;
  return `${progress.phase} ${count}`;
}
