import type { Client } from "./client.js";
import { guiAgentGroupItems, guiAgentTreeOrder } from "./gui-agent-tree.js";
import type { GuiAction } from "./gui-types.js";
import type {
  GuiAgent,
  GuiAgentActResult,
  GuiAgentControl,
  GuiAgentKind,
  GuiAgentObservation,
  GuiAgentOptions,
  GuiAgentQuery,
  GuiAgentSnapshot,
  GuiAgentStepResult,
  GuiAgentTarget,
} from "./gui-agent-types.js";
import type { EntitySnapshot, LifecycleTargetWatch } from "./types.js";

// Match the native CONTROL_COMPONENTS precedence before the separate group component.
const nativeKinds: Readonly<Record<string, GuiAgentKind>> = {
  GuiButton: "button",
  GuiCheckbox: "checkbox",
  GuiSlider: "slider",
  GuiTextInput: "textInput",
  GuiScrollView: "scrollView",
  GuiVirtualList: "virtualList",
  GuiColor: "color",
  GuiInputTarget: "inputTarget",
  GuiGroup: "group",
};

const stateFields: Record<GuiAgentKind, readonly string[]> = {
  button: ["selected"],
  checkbox: ["checked"],
  slider: ["value", "upper", "range", "min", "max", "step", "axis"],
  textInput: ["text", "masked", "numeric", "value", "min", "max", "step"],
  color: ["hue", "saturation", "value", "alpha", "alpha_rail"],
  scrollView: ["offset_x", "offset_y", "capacity_x", "capacity_y"],
  virtualList: [
    "offset_x",
    "offset_y",
    "capacity_x",
    "capacity_y",
    "item_count",
    "range_first",
    "range_last",
  ],
  group: ["axis", "selection"],
  inputTarget: [],
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function actions(
  kind: GuiAgentKind,
  state: GuiAgentControl["state"],
): GuiAction["kind"][] {
  switch (kind) {
    case "button":
      return ["press"];
    case "checkbox":
      return ["toggle"];
    case "slider":
      return ["scalar"];
    case "textInput":
      return [state.numeric ? "scalar" : "text", "submit"];
    case "color":
      return ["color"];
    case "scrollView":
      return ["scrollTo", "scrollBy"];
    case "virtualList":
      return ["scrollTo", "scrollBy", "scrollToIndex"];
    case "inputTarget":
    case "group":
      return [];
  }
}

/** No state authority: every call reads or submits through supplied live World clients. */
export function createGuiAgent(
  worlds: Readonly<Record<string, Client>>,
  options: GuiAgentOptions = {},
): GuiAgent {
  const clients = new Map(Object.entries(worlds));
  const scope = crypto.randomUUID();
  const maxEntities = options.maxEntities ?? 1000;
  if (!Number.isInteger(maxEntities) || maxEntities < 1 || maxEntities > 10_000)
    throw new RangeError("maxEntities must be an integer in 1..10000");
  for (const [key, client] of clients) {
    if (!/^[a-zA-Z0-9_-]+$/.test(key) || !client.worldReference)
      throw new Error(
        "Each World needs a unique simple key and a ready client",
      );
  }

  function reference(
    key: string,
    entity: bigint,
    component: number,
    incarnation: bigint,
  ): string {
    const client = clients.get(key)!;
    const world = client.worldReference!;
    return [
      scope,
      key,
      world.id,
      world.incarnation,
      client.session,
      entity,
      component,
      incarnation,
    ].join("/");
  }

  function resolve(ref: string) {
    const pieces = ref.split("/");
    if (pieces.length !== 8 || pieces[0] !== scope) return null;
    const client = clients.get(pieces[1]!);
    if (!client) return null;
    const world = client.worldReference!;
    if (
      pieces[2] !== String(world.id) ||
      pieces[3] !== String(world.incarnation) ||
      pieces[4] !== String(client.session)
    )
      return null;
    if (!pieces.slice(5).every((part) => /^(0|[1-9][0-9]{0,19})$/.test(part)))
      return null;
    const entity = BigInt(pieces[5]!);
    const component = Number(pieces[6]);
    const incarnation = BigInt(pieces[7]!);
    if (
      entity > 0xffffffffffffffffn ||
      incarnation > 0xffffffffffffffffn ||
      component > 0xffff
    )
      return null;
    return { client, entity, component, incarnation };
  }

  async function observe(
    key: string,
    client: Client,
  ): Promise<GuiAgentSnapshot> {
    const world = client.worldReference!;
    const observation: GuiAgentObservation = {
      world: {
        key,
        id: String(world.id),
        incarnation: String(world.incarnation),
        session: String(client.session),
        schemaHash: String(client.schemaHash),
      },
      status: "observed",
      firstTick: null,
      lastTick: null,
      truncated: false,
      invalidated: 0,
      error: null,
    };
    let watch: LifecycleTargetWatch | undefined;
    const changed = new Set<string>();
    const stamp = (tick: bigint) => {
      observation.firstTick ??= String(tick);
      observation.lastTick = String(tick);
    };
    const identity = (entity: bigint, component: number) =>
      `${entity}/${component}`;
    const kinds = new Map(
      Object.entries(nativeKinds).flatMap(([name, kind]) => {
        const descriptor = client.components[name];
        return descriptor ? [[descriptor.id, kind] as const] : [];
      }),
    );
    async function read() {
      const entities = new Map<
        bigint,
        { entity: EntitySnapshot; tick: bigint }
      >();
      let after = 0n;
      do {
        const page = await client.inspectPage({
          collection: "entities",
          after,
          limit: Math.min(100, maxEntities - entities.size),
        });
        stamp(page.tick);
        for (const entity of page.entities)
          entities.set(entity.id, { entity, tick: page.tick });
        after = page.next;
      } while (after !== 0n && entities.size < maxEntities);
      observation.truncated ||= after !== 0n;
      return entities;
    }
    try {
      const initial = await read();
      const targets = [...initial.values()].flatMap(({ entity }) =>
        entity.components
          .filter((component) => kinds.has(component.component))
          .map((component) => ({
            target: {
              kind: "component" as const,
              entity: entity.id,
              component: component.component,
            },
            kinds: 8 | 32 | 64,
          })),
      );
      if (!targets.length) return { controls: [], observations: [observation] };
      watch = await client.watchLifecycle(targets, (event) => {
        if (
          event.kind === "event" &&
          event.observation.kind === "component" &&
          event.observation.change !== "updated"
        )
          changed.add(
            identity(event.observation.entity, event.observation.component),
          );
      });
      const entities = await read();
      const tree = await guiAgentTreeOrder(
        Array.from(entities.values(), ({ entity }) => entity),
        client,
        maxEntities,
        observation,
      );
      // Removal ACK follows every lifetime event from this observation window.
      await watch.remove();
      const controls: GuiAgentControl[] = [];
      const watched = new Set(
        targets.map(({ target }) => identity(target.entity, target.component)),
      );
      // The second read may discover additions that had no lifetime baseline.
      // Omitting those is incomplete discovery, especially for label uniqueness.
      for (const { entity } of entities.values()) {
        for (const component of entity.components) {
          if (
            kinds.has(component.component) &&
            !watched.has(identity(entity.id, component.component))
          )
            observation.invalidated++;
        }
      }
      for (const baseline of watch.baselines) {
        const { target, lifetime } = baseline;
        if (target.kind !== "component") continue;
        const current = entities.get(target.entity);
        const component = current?.entity.components.find(
          (entry) => entry.component === target.component,
        );
        if (
          changed.has(identity(target.entity, target.component)) ||
          lifetime.kind !== "component" ||
          lifetime.incarnation === null ||
          !component ||
          !current
        ) {
          observation.invalidated++;
          continue;
        }
        const kind = kinds.get(target.component)!;
        const behavior = current.entity.components.find(
          (entry) => entry.component === client.components.GuiBehavior?.id,
        )?.fields;
        const state: GuiAgentControl["state"] = {};
        for (const name of stateFields[kind]) {
          const value = component.fields[name];
          if (
            typeof value === "string" ||
            typeof value === "number" ||
            typeof value === "boolean"
          )
            state[name] = value;
        }
        if (state.masked) {
          state.text = null;
          state.value = null;
        }
        const semantic = behavior?.semantic_label;
        const label = semantic || component.fields.label || "";
        const enabled = behavior ? behavior.effective_enabled === true : null;
        const visible = behavior ? behavior.effective_visible === true : null;
        const available = behavior ? behavior.available === true : null;
        const focusParts =
          kind === "group" || behavior?.focusable !== true
            ? []
            : Array.from(
                {
                  length:
                    kind === "slider" && state.range
                      ? 2
                      : kind === "color"
                        ? state.alpha_rail
                          ? 3
                          : 2
                        : 1,
                },
                (_, index) => index,
              );
        const supported =
          kind === "group"
            ? []
            : [
                ...actions(kind, state),
                ...(focusParts.length ? ["focus" as const] : []),
                "blur" as const,
              ];
        controls.push({
          ref: reference(
            key,
            target.entity,
            target.component,
            lifetime.incarnation,
          ),
          world: key,
          entity: String(target.entity),
          component: target.component,
          incarnation: String(lifetime.incarnation),
          kind,
          label: String(label),
          parent:
            current.entity.link.parent === null
              ? null
              : String(current.entity.link.parent),
          group: null,
          tick: String(current.tick),
          enabled,
          visible,
          available,
          actions: enabled && visible && available ? supported : [],
          focusParts,
          state,
        });
      }
      return {
        controls: guiAgentGroupItems(tree, controls, kinds, observation),
        observations: [observation],
      };
    } catch (error) {
      // A paged subscription can fail after acquiring a prefix. Release that prefix.
      if (error && typeof error === "object" && "partial" in error)
        watch = (error as { partial: LifecycleTargetWatch }).partial;
      observation.status = "unavailable";
      observation.error = message(error);
      return { controls: [], observations: [observation] };
    } finally {
      if (watch) await watch.remove().catch(() => {});
    }
  }

  async function snapshot(
    filter: { world?: string } = {},
  ): Promise<GuiAgentSnapshot> {
    if (filter.world !== undefined && !clients.has(filter.world))
      throw new Error("Unknown agent World key");
    const result: GuiAgentSnapshot = { controls: [], observations: [] };
    for (const [key, client] of clients) {
      if (filter.world !== undefined && key !== filter.world) continue;
      const found = await observe(key, client);
      result.controls.push(...found.controls);
      result.observations.push(...found.observations);
    }
    return result;
  }

  async function query(filter: GuiAgentQuery = {}): Promise<GuiAgentSnapshot> {
    const result = await snapshot(filter);
    result.controls = result.controls.filter(
      (control) =>
        (filter.kind === undefined || control.kind === filter.kind) &&
        (filter.label === undefined || control.label === filter.label) &&
        (filter.within === undefined || control.group === filter.within),
    );
    return result;
  }

  async function target(selection: GuiAgentTarget) {
    if ("ref" in selection)
      return { ref: selection.ref, error: null, matches: [] };
    const found = await query(selection);
    const matches = found.controls.map((entry) => entry.ref);
    if (
      found.observations.some(
        (entry) =>
          entry.status !== "observed" || entry.truncated || entry.invalidated,
      )
    )
      return { ref: null, error: "IncompleteObservation", matches };
    return matches.length === 1
      ? { ref: matches[0]!, error: null, matches }
      : {
          ref: null,
          error: matches.length ? "AmbiguousTarget" : "TargetNotFound",
          matches,
        };
  }

  return {
    snapshot,
    query,
    async act({ steps }): Promise<GuiAgentActResult> {
      if (!Array.isArray(steps) || steps.length > 100)
        throw new RangeError("An act request accepts at most 100 steps");
      const results: GuiAgentStepResult[] = [];
      const observationErrors: string[] = [];
      let stopped = false;
      for (const [index, step] of steps.entries()) {
        const result: GuiAgentStepResult = {
          index,
          ref: null,
          status: "notRun",
          batch: null,
          error: null,
          matches: [],
        };
        results.push(result);
        if (stopped) continue;
        result.status = "rejected";
        try {
          const selected = await target(step.target);
          Object.assign(result, selected);
          const exact = selected.ref === null ? null : resolve(selected.ref);
          if (!exact || exact.client.closure) {
            result.error ??= exact ? "SessionClosed" : "StaleReference";
            stopped = true;
            continue;
          }
          // The runtime revalidates current eligibility and exact incarnation.
          result.status = "unknown";
          const outcome = await exact.client.batch([
            {
              kind: "guiAction",
              entity: { kind: "handle", id: exact.entity },
              component: exact.component,
              incarnation: exact.incarnation,
              action: step.action,
            },
          ]);
          result.batch = {
            id: String(outcome.batchId),
            tick: String(outcome.tick),
            ok: outcome.ok,
            error: outcome.ok ? null : outcome.error,
          };
          result.status = outcome.ok
            ? "acknowledged"
            : outcome.error.scope === "commit"
              ? "failed"
              : "rejected";
          result.error = outcome.ok ? null : outcome.error.reason;
          stopped = !outcome.ok;
          try {
            await options.afterAction?.(exact.client, outcome);
          } catch (error) {
            observationErrors.push(message(error));
            stopped = true;
          }
        } catch (error) {
          // Generated support modules are separate copies: use the public code,
          // never constructor identity, for definitely unapplied requests.
          const code =
            error && typeof error === "object" && "code" in error
              ? error.code
              : undefined;
          if (
            code === "IPP_REQUEST_NOT_SENT" ||
            code === "IPP_REQUEST_REJECTED"
          ) {
            result.status = "rejected";
            result.error = code;
          } else {
            result.error = message(error);
          }
          stopped = true;
        }
      }
      const observed = await snapshot();
      return {
        ok: results.every((step) => step.status === "acknowledged"),
        steps: results,
        observed,
        observationErrors,
      };
    },
  };
}
