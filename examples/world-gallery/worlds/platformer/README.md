# Platformer trail

This gallery world loads a Blender-authored KayKit track, character and presentation from a saved World generated during the gallery build. The application turns the companion route metadata into Host-driven movement and combines it with exported skeletal clips.

The source mannequin uses one gray material and no image textures; its panel and bolt details are modeled geometry.

Walk, Run and Crawl use phase-matched transitions. A cold destination prepares before the transition, so the current gait and route keep advancing while bytes arrive.

The route blends authored headings around corners. Reverse changes the signed route clock while the gait continues forward and a separate Host transition turns the rig, leaving the following camera, light and orb attached to the route root. See [session.tsx](session.tsx) for playback composition and [scene-file.ts](scene-file.ts) for saved-World initialization.

This is a route-driven animation example. The original flat-ground gaits have no foot IK or slope adaptation, so feet can slip or intersect a ramp. Collision handling and jumping are outside its scope. The packed Blender source, rebuild instructions, licenses and third-party attribution live under [authoring](authoring/PROVENANCE.md).
