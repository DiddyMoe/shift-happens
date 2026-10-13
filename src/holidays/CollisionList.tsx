import type { CollisionPreview } from "../lib/holidays";

/** Dates in the year that more than one active holiday falls on. */
export function CollisionList({
  year,
  collisions,
}: {
  year: number;
  collisions: CollisionPreview;
}) {
  const count = collisions.collision_count;
  const unlisted = collisions.omitted_diagnostic_count;
  return (
    <section aria-labelledby="collisions-title">
      <h2 id="collisions-title">Holiday collisions</h2>
      {count === 0 ? (
        <p className="status-line" role="status">
          No two active holidays share a date in {year}.
        </p>
      ) : (
        <div className="notice notice-warn" role="alert">
          <p>
            <span className="display">{count}</span>{" "}
            {count === 1 ? "date has" : "dates have"} more than one active
            holiday. Resolve each before generating a schedule.
          </p>
          <ul className="bullets hol-collisions">
            {collisions.collisions.map((c) => (
              <li key={c.date}>
                <strong className="display">{c.date}</strong>:{" "}
                {c.holidays.map((h) => h.name).join(", ")}
                {c.omitted_source_id_count > 0 &&
                  ` and ${c.omitted_source_id_count} more`}
              </li>
            ))}
          </ul>
          {unlisted > 0 && (
            <p>
              {unlisted} more {unlisted === 1 ? "date isn’t" : "dates aren’t"}{" "}
              listed. Resolve these first, then check again.
            </p>
          )}
        </div>
      )}
    </section>
  );
}
