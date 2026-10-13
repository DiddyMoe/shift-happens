import { HowItWorks } from "../RecordControls";
import type { RotationPreview as Rotation } from "../lib/holidays";
import { groupLabel } from "./labels";

/** Which employee group works each holiday set in the year. */
export function RotationPreview({ rotation }: { rotation: Rotation }) {
  return (
    <section aria-labelledby="rotation-title">
      <h2 id="rotation-title">Rotation for {rotation.year}</h2>
      <dl className="facts">
        <div>
          <dt>Set A</dt>
          <dd>Employee group {groupLabel(rotation.set_a_group)}</dd>
        </div>
        <div>
          <dt>Set B</dt>
          <dd>Employee group {groupLabel(rotation.set_b_group)}</dd>
        </div>
      </dl>
      <HowItWorks label="How the rotation works">
        <p>
          Groups swap every year, counting from{" "}
          <span className="display">{rotation.start_year}</span>. Holidays with
          no set don&rsquo;t need a particular group, and employees with no
          group match neither set.
        </p>
      </HowItWorks>
    </section>
  );
}
