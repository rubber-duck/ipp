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
