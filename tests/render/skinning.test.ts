import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { invoke, recordCapture } from "./evidence.js";

const workspace = resolve(process.cwd());
for (const configuration of [
  "render-skeletal-animation",
  "render-expanded",
] as const) {
  const directory = resolve(workspace, "target/browser-build", configuration);
  const build: BrowserBuildConfiguration = {
    name: configuration,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };

  test(`${configuration}: built-in rigs deform independently through generated client, WASM and WebGL, and recover`, {
    timeout: 60_000,
  }, async (context) => {
    await runBrowserEnvironment(
      "skinning",
      {
        workspace,
        build,
        mismatchBuild: {
          ...build,
          name: "render",
          generatedModule: resolve(
            workspace,
            "target/browser-build/render/generated.js",
          ),
          runtimeWasm: resolve(
            workspace,
            "target/browser-build/render/runtime.wasm",
          ),
          exportWasm: resolve(
            workspace,
            "target/browser-build/render/export.wasm",
          ),
          contractArtifact: resolve(
            workspace,
            "target/browser-build/render/contract.bin",
          ),
        },
      },
      context.signal,
      async (environment) => {
        const module = `${environment.urls.origin}/dist/tests/render/skinning-fixture.js`;
        const call = <T>(name: string, args: readonly unknown[] = []) =>
          environment.execute(name, args, () =>
            invoke<T>(environment.page, module, name, args),
          );
        const captured = new Set<string>();
        const capture = async (label: string, draws = 2) => {
          const report = await call<{ drawCalls: number; triangles: number }>(
            "capture",
            [label, draws],
          );
          await recordCapture(
            environment.page,
            module,
            environment.evidence.directory,
            captured,
            label,
            { canvasSelector: "#skinning-canvas" },
          );
          await environment.evidence.record(label, report);
          return report;
        };
        try {
          await call("initialize", [
            {
              generatedModuleUrl: environment.urls.generated,
              workerScriptUrl: environment.urls.workerScript,
              wasmUrl: environment.urls.wasm,
            },
          ]);
          const rest = await capture("rest");
          assert.equal(rest.drawCalls, 2);
          assert.equal(rest.triangles, 32);
          const blue = await call<number[]>("pixel", ["rest", 120, 70]);
          assert.ok(
            blue[2]! > 200 && blue[0]! < 110,
            "rest tip lies at the analytically projected point",
          );
          await call("pose", [0, "bent"]);
          await capture("bent");
          assert.deepEqual(
            await call("pixel", ["bent", 120, 70]),
            [10, 14, 20],
            "tip leaves its rest position",
          );
          const bentTip = await call<number[]>("pixel", ["bent", 45, 150]);
          assert.ok(
            bentTip[2]! > 200 && bentTip[0]! < 130,
            "tip rotates around the upper joint pivot",
          );
          assert.deepEqual(
            await call("pixel", ["rest", 120, 225]),
            await call("pixel", ["bent", 120, 225]),
            "root-only vertices remain fixed",
          );
          assert.ok(
            (await call<number>("regionDifference", ["rest", "bent", 0, 200])) >
              2000,
            "left rig visibly bends",
          );
          assert.equal(
            await call<number>("regionDifference", ["rest", "bent", 200, 400]),
            0,
            "shared rig source leaves right instance unchanged",
          );
          const invalid = await call<{ scalar: number }>("applyInvalidPose");
          assert.equal(
            invalid.scalar,
            123,
            "The preceding component insert remains applied",
          );
          await capture("invalid-pose", 1);
          assert.ok(
            (await call<number>("regionDifference", [
              "bent",
              "invalid-pose",
              0,
              200,
            ])) > 2000,
            "Invalid applied pose suppresses only the affected rig",
          );
          assert.equal(
            await call<number>("regionDifference", [
              "bent",
              "invalid-pose",
              200,
              400,
            ]),
            0,
            "The other rig keeps rendering",
          );
          await call("pose", [0, "bent"]);
          await capture("corrected-pose");
          assert.equal(
            await call<number>("regionDifference", [
              "bent",
              "corrected-pose",
              0,
              400,
            ]),
            0,
            "A corrected pose restores the deformed image",
          );
          await call("pose", [1, "override"]);
          await capture("both-bent");
          assert.ok(
            (await call<number>("regionDifference", [
              "bent",
              "both-bent",
              200,
              400,
            ])) > 2000,
            "direct joint override deforms second instance",
          );
          await call("recover");
          await capture("recovered");
          assert.equal(
            await call<number>("regionDifference", [
              "both-bent",
              "recovered",
              0,
              400,
            ]),
            0,
            "context restoration preserves every deformed pixel",
          );
          await call("pose", [0, "rest"]);
          await call("pose", [1, "rest"]);
          await capture("restored-rest");
          assert.equal(
            await call<number>("regionDifference", [
              "rest",
              "restored-rest",
              0,
              400,
            ]),
            0,
            "withdrawal restores rest pose exactly",
          );
          if (configuration === "render-expanded") {
            const state = await call<{
              controllers: { time: number; state: string }[];
            }>("animationScene");
            assert.deepEqual(
              state.controllers.map((controller) => [
                controller.time,
                controller.state,
              ]),
              [
                [1, "paused"],
                [2, "paused"],
              ],
            );
            await capture("animated");
            assert.ok(
              (await call<number>("regionDifference", [
                "rest",
                "animated",
                0,
                200,
              ])) > 1000,
            );
            assert.equal(
              await call<number>("regionDifference", [
                "both-bent",
                "animated",
                200,
                400,
              ]),
              0,
              "pose keyframe endpoint matches the explicit joint override",
            );
            const transitions = await call<{
              walkRun: {
                time: number;
                transition?: {
                  elapsed: number;
                  duration: number;
                  easing: string;
                  pending: boolean;
                };
              };
              waveRun: {
                time: number;
                transition?: {
                  elapsed: number;
                  duration: number;
                  easing: string;
                  pending: boolean;
                };
              };
            }>("animationTransitions");
            assert.deepEqual(
              [
                transitions.walkRun.transition?.duration,
                transitions.walkRun.transition?.easing,
                transitions.walkRun.transition?.pending,
              ],
              [2, "smoothstep", false],
            );
            assert.deepEqual(
              [
                transitions.waveRun.transition?.duration,
                transitions.waveRun.transition?.easing,
                transitions.waveRun.transition?.pending,
              ],
              [2, "linear", false],
            );
            assert.ok(
              Math.abs(transitions.walkRun.time - 0.5) < 1e-6,
              "MatchPhase did not map the quarter phase from four to two seconds",
            );
            assert.equal(
              transitions.waveRun.time,
              2,
              "Preserve did not retain the partial wave endpoint time",
            );
            await capture("walk-wave-to-run");
            assert.ok(
              (await call<number>("regionDifference", [
                "animated",
                "walk-wave-to-run",
                0,
                200,
              ])) > 100,
              "walk-to-run transition did not visibly blend the first rig",
            );
            assert.ok(
              (await call<number>("regionDifference", [
                "animated",
                "walk-wave-to-run",
                200,
                400,
              ])) > 100,
              "partial-wave-to-run transition did not blend added joint coverage",
            );
            const interrupted = await call<{
              transition?: { elapsed: number; easing: string };
            }>("interruptAnimationTransition");
            assert.equal(interrupted.transition?.elapsed, 0);
            assert.equal(interrupted.transition?.easing, "smoothstep");
            await capture("interrupted-transition");
            assert.equal(
              await call<number>("regionDifference", [
                "walk-wave-to-run",
                "interrupted-transition",
                0,
                200,
              ]),
              0,
              "transition interruption did not preserve the rendered composite",
            );
            await call("completeAnimationTransitions");
            await capture("transition-final-run");
            await call("explicitTransitionPoses", [
              0.3,
              -Math.PI / 2,
              0.3,
              -Math.PI / 2,
            ]);
            await capture("direct-run");
            assert.equal(
              await call<number>("regionDifference", [
                "transition-final-run",
                "direct-run",
                0,
                400,
              ]),
              0,
              "completed transitions did not match the direct run pose",
            );
            const walkProgress = transitions.walkRun.transition!.elapsed / 2;
            const walkWeight =
              walkProgress * walkProgress * (3 - 2 * walkProgress);
            const walkSourceAngle =
              (Math.min(2, 1 + transitions.walkRun.transition!.elapsed) *
                Math.PI) /
              4;
            const waveWeight = transitions.waveRun.transition!.elapsed / 2;
            await call("explicitTransitionPoses", [
              0.3 * walkWeight,
              walkSourceAngle * (1 - walkWeight) - (Math.PI * walkWeight) / 2,
              0.3 * waveWeight,
              Math.PI / 2 - Math.PI * waveWeight,
            ]);
            await capture("transition-pose-oracle");
            assert.ok(
              (await call<number>("regionDifference", [
                "walk-wave-to-run",
                "transition-pose-oracle",
                0,
                400,
              ])) < 50,
              "rendered transition disagreed with the independently calculated joint poses",
            );
            await call("animationScene");
            await capture("animation-reset");
            assert.equal(
              await call<number>("regionDifference", [
                "animated",
                "animation-reset",
                0,
                400,
              ]),
              0,
              "transition checks did not restore the original animation fixture",
            );
            await call("animationWeight", [0.5]);
            await capture("weighted");
            assert.equal(
              await call<number>("regionDifference", [
                "animated",
                "weighted",
                0,
                160,
              ]),
              0,
              "changing the second controller leaves the first instance unchanged",
            );
            await call("animationWeight", [0.5, true]);
            await capture("additive");
            assert.equal(
              await call<number>("regionDifference", [
                "weighted",
                "additive",
                0,
                400,
              ]),
              0,
              "additive half-rotation from the rest reference matches the weighted base pose",
            );
            await call("recover");
            await capture("animated-recovered");
            assert.equal(
              await call<number>("regionDifference", [
                "additive",
                "animated-recovered",
                0,
                400,
              ]),
              0,
            );
            await call("animationStop", [1]);
            await call("explicitAngle", [1, Math.PI / 4]);
            await capture("explicit-half");
            assert.equal(
              await call<number>("regionDifference", [
                "weighted",
                "explicit-half",
                0,
                400,
              ]),
              0,
              "weighted and interpolated pose frames agree with an analytic 45-degree joint rotation",
            );
            await call("replaceAnimatedSkeleton");
            await call("pose", [1, "rest"]);
            await capture("animation-withdrawn");
            assert.equal(
              await call<number>("regionDifference", [
                "rest",
                "animation-withdrawn",
                0,
                400,
              ]),
              0,
              "replacement withdraws playback and exposes the replacement rest pose",
            );
          }
          if (configuration === "render-expanded") {
            await call("authoredNormals");
            await call("pose", [0, "bent"]);
            await call("lightingScene");
            await capture("lit-skinned", 3);
            await call("receiveShadows", [false]);
            await capture("lit-unshadowed", 3);
            assert.ok(
              (await call<number>("regionDifference", [
                "lit-skinned",
                "lit-unshadowed",
                0,
                400,
              ])) > 100,
              "deformed rigs cast measurable shadows on the receiver",
            );
            await call("receiveShadows", [true]);
            await call("recover", [3]);
            await capture("lit-recovered", 3);
            assert.equal(
              await call<number>("regionDifference", [
                "lit-skinned",
                "lit-recovered",
                0,
                400,
              ]),
              0,
              "lit deformation and shadows recover exactly",
            );
            await call("bakeBentRig");
            await capture("lit-baked", 3);
            assert.ok(
              (await call<number>("regionDifference", [
                "lit-skinned",
                "lit-baked",
                0,
                400,
              ])) < 30,
              "GPU geometry and shadow silhouette match the independently baked rigid mesh",
            );
            return;
          }
          const uploads = await call<{ rejected: { status: string } }>(
            "uploadedFourJointScene",
          );
          assert.equal(
            uploads.rejected.status,
            "failed",
            "malformed uploaded hierarchy rejects",
          );
          await capture("four-joints", 1);
          assert.deepEqual(
            await call("pixel", ["four-joints", 210, 138]),
            [0, 255, 0],
            "all four distinct joint transforms contribute after weight normalization",
          );
          assert.deepEqual(
            await call("pixel", ["four-joints", 210, 250]),
            [10, 14, 20],
            "deformed square leaves the rest position",
          );
        } finally {
          await call("close");
        }
      },
    );
  });
}
