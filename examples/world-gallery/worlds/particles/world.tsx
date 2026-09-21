import {
  Entity,
  Transform,
  MeshInstance,
  UnlitMaterial,
  ParticleEmitter,
  ParticleSprite,
  ParticleMesh,
} from "@ipp/react";
import { World } from "@ipp/react/web";
import { hexToLinear } from "../../shared/colors.js";
import type { ParticleSettings } from "./controls.js";

/** React authors one persistent emitter; the Host owns every simulation step. */
export function ParticlesWorld({ settings }: { settings: ParticleSettings }) {
  const color = hexToLinear(settings.color);
  return (
    <World>
      <Entity id="particle-plinth">
        <Transform y={-1.9} />
        <MeshInstance source="ipp://mesh/cube?width=2.4&height=0.16&length=2.4" />
        <UnlitMaterial r={0.025} g={0.045} b={0.065} />
      </Entity>
      <Entity id="particle-nozzle">
        <Transform y={-1.72} />
        <MeshInstance source="ipp://mesh/cube?width=0.32&height=0.22&length=0.32" />
        <UnlitMaterial r={0.15} g={0.25} b={0.3} />
      </Entity>
      <Entity id="particle-fountain">
        <Transform y={-1.58} />
        <ParticleEmitter
          enabled={settings.emitting}
          restart={settings.restart}
          seed={42}
          capacity={8000}
          rate={settings.rate}
          lifetime={settings.lifetime}
          lifetime_random={0.25}
          speed={settings.speed}
          speed_random={0.2}
          spread={settings.spread}
          size={settings.size}
          size_random={0.45}
          rotation_random={Math.PI}
          spin={2.4}
          acceleration_y={-4.5}
        />
        {settings.presentation === "sprites" ? (
          <ParticleSprite
            r={color[0]}
            g={color[1]}
            b={color[2]}
            blend={1}
            end_size={0.2}
          />
        ) : (
          <>
            <ParticleMesh source="ipp://mesh/cube?width=1&height=1&length=1" />
            <UnlitMaterial r={color[0]} g={color[1]} b={color[2]} />
          </>
        )}
      </Entity>
    </World>
  );
}
