in vec2 shieldUv;
in vec2 shieldSize;
in float shieldFace;

vec4 materialFragment() {
  // Open against the panel, with four visible walls closing the side gap.
  if (shieldFace < -0.5) return vec4(0.0);
  vec2 metres = shieldUv * shieldSize;
  if (abs(shieldFace) < 0.5) {
    float stripe = step(0.5, fract((metres.x + metres.y) / 0.08));
    return vec4(p_color.rgb * mix(0.3, 1.0, stripe), 1.0);
  }
  // Metres across the front cap and the distance to its nearest edge.
  vec2 edges = min(metres, shieldSize - metres);
  float edge = min(edges.x, edges.y);

  // A hazard-striped frame around the edge.
  if (edge < 0.024) {
    float stripe = step(0.5, fract((metres.x + metres.y) / 0.08));
    return vec4(p_color.rgb * mix(0.3, 1.0, stripe), 1.0);
  }

  // Sparse diagonal hatching across the glass while armed. Everything else
  // is cut out, so the panel shows through between the marks.
  float hatch = fract((metres.x - metres.y) / 0.09);
  if (p_hatch > 0.5 && hatch < 0.15) return vec4(p_color.rgb * 0.8, 1.0);
  return vec4(0.0);
}
