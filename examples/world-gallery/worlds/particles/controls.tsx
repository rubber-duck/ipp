export interface ParticleSettings {
  presentation: "sprites" | "meshes";
  emitting: boolean;
  restart: number;
  rate: number;
  lifetime: number;
  speed: number;
  spread: number;
  size: number;
  color: string;
}

export const INITIAL_PARTICLES: ParticleSettings = {
  presentation: "sprites",
  emitting: true,
  restart: 0,
  rate: 900,
  lifetime: 2.2,
  speed: 5,
  spread: 0.45,
  size: 0.09,
  color: "#55ddff",
};

export function ParticleControls({
  settings,
  update,
}: {
  settings: ParticleSettings;
  update(patch: Partial<ParticleSettings>): void;
}) {
  return (
    <>
      <div className="panel-heading">
        <span className="step">World 04</span>
        <h2>Particle fountain</h2>
        <p>
          Shape the spray, switch its appearance, or stop emission and watch the
          last particles fade.
        </p>
      </div>
      <label className="mesh-select" htmlFor="particle-presentation">
        <span>Appearance</span>
        <select
          id="particle-presentation"
          value={settings.presentation}
          onChange={(event) =>
            update({
              presentation: event.currentTarget
                .value as ParticleSettings["presentation"],
            })
          }
        >
          <option value="sprites">Glowing sprites</option>
          <option value="meshes">Tumbling cubes</option>
        </select>
      </label>
      <label className="color-control" htmlFor="particle-color">
        <span>Particle color</span>
        <input
          id="particle-color"
          type="color"
          value={settings.color}
          onChange={(event) => update({ color: event.currentTarget.value })}
        />
      </label>
      <fieldset>
        <legend>Emission</legend>
        {(
          [
            ["rate", "Particles per second", 50, 1800, 50, "/s"],
            ["lifetime", "Lifetime", 0.5, 4, 0.1, "s"],
            ["speed", "Launch speed", 1, 8, 0.1, "m/s"],
            ["spread", "Spread", 0.05, 1.2, 0.05, "rad"],
            ["size", "Size", 0.03, 0.22, 0.01, "m"],
          ] as const
        ).map(([field, label, min, max, step, unit]) => (
          <label
            className="range-control"
            htmlFor={`particle-${field}`}
            key={field}
          >
            <span>
              {label}{" "}
              <small>
                {settings[field]} {unit}
              </small>
            </span>
            <input
              id={`particle-${field}`}
              type="range"
              min={min}
              max={max}
              step={step}
              value={settings[field]}
              onChange={(event) =>
                update({ [field]: Number(event.currentTarget.value) })
              }
            />
          </label>
        ))}
        <p className="field-note">
          Emission settings apply to new particles. The fountain has gravity;
          particles pass through the base.
        </p>
      </fieldset>
      <button
        id="particle-emitting"
        type="button"
        className="secondary-button"
        aria-pressed={settings.emitting}
        onClick={() => update({ emitting: !settings.emitting })}
      >
        {settings.emitting ? "Stop emission" : "Start emission"}
      </button>
      <button
        id="particle-restart"
        type="button"
        className="secondary-button"
        onClick={() =>
          update({ restart: settings.restart + 1, emitting: true })
        }
      >
        Restart fountain
      </button>
    </>
  );
}
