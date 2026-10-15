import {
  defineClient,
  type ClientContext,
  type LoadedModule,
} from "../../../tools/shared-host/client.js";
import { crop } from "../../../tools/shared-host/images.js";
import { SpecimenSession } from "../skin-lab/specimen-session.js";
import type { SkinSpecimen } from "../skin-lab/specimen.js";

type State = { path: string; identity: string; session: SpecimenSession };
async function openSession(context: ClientContext, loaded: LoadedModule) {
  const createSpecimen = loaded.module.default as (
    contract: ClientContext["contract"],
  ) => SkinSpecimen;
  return SpecimenSession.open({
    host: context.host,
    contract: context.contract as unknown as Parameters<
      typeof SpecimenSession.open
    >[0]["contract"],
    font: await context.font(),
    specimen: createSpecimen(context.contract),
    themes: {},
    world: `glass-reference/${context.name}`,
  });
}

/** Recreate the specimen on source edits; each capture still uses real native state pins. */
export default defineClient<State>({
  reloadOnCapture: false,
  async open(context, args) {
    const family = args[0];
    if (
      !family ||
      ![
        "actions",
        "switch",
        "switch-scale",
        "switch-light",
        "slider",
        "checkbox",
        "surfaces",
      ].includes(family)
    )
      throw new Error(
        "Choose actions, switch, switch-scale, switch-light, slider, checkbox or surfaces",
      );
    const path = `tests/gui/glass-skin/specimens/${family}-reference.tsx`;
    const loaded = await context.load(path);
    if (!loaded) throw new Error(`Missing reference specimen ${family}`);
    return {
      path,
      identity: loaded.identity,
      session: await openSession(context, loaded),
    };
  },
  async capture(state, context, args) {
    const loaded = await context.load(state.path, state.identity);
    if (loaded) {
      await state.session.close();
      state.session = await openSession(context, loaded);
      state.identity = loaded.identity;
    }
    await state.session.assetSettled("ipp-glass/shader");
    const captured = await state.session.capture(
      context.present,
      args.length ? args : undefined,
    );
    return {
      images: {
        sheet: captured.image,
        ...Object.fromEntries(
          captured.states.map((s) => [s.name, crop(s.image, s.cell)]),
        ),
      },
      report: {
        states: captured.states.map(({ name, routing }) => ({ name, routing })),
        leftover: captured.leftover,
      },
    };
  },
  async close(state) {
    await state.session.close();
  },
});
