import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type {
  GuiNativeTextState,
  GuiPhysicalContext,
  GuiTarget,
} from "@ipp/client";
import { Children, Entity, PresentationGate } from "../../components.js";
import { Style } from "../../gui/components.js";
import { Resources } from "../../resources.js";
import { useEntityId } from "../../scope/identity.js";
import { OptionList, type SelectOption } from "../selection/option-list.js";
import { Floating } from "./overlay.js";

export interface TriggerMatch {
  readonly trigger: string;
  readonly query: string;
  readonly start: number;
  readonly end: number;
}
export interface TriggerOption extends SelectOption {
  readonly insert: string;
}
export interface TriggerMenuProps {
  readonly id?: string;
  readonly context: GuiPhysicalContext;
  readonly target: GuiTarget | null;
  readonly triggers: readonly string[];
  readonly candidates: (
    match: TriggerMatch,
    signal: AbortSignal,
  ) => readonly TriggerOption[] | Promise<readonly TriggerOption[]>;
  readonly onInsert?: (option: TriggerOption) => void;
  readonly onError?: (error: Error) => void;
  readonly width?: number;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** Trigger and query must start a whitespace-delimited word. Offsets index UTF-8 bytes. */
export function matchTextTrigger(
  state: GuiNativeTextState,
  triggers: readonly string[],
): TriggerMatch | null {
  if (
    state.composition ||
    state.clientInput ||
    state.readOnly ||
    state.masked ||
    state.purpose !== undefined ||
    state.selectionStart !== state.selectionEnd
  )
    return null;
  const prefix = decoder.decode(
    encoder.encode(state.text).subarray(0, state.selectionEnd),
  );
  const word = prefix.match(/[^\s]+$/u)?.[0];
  if (!word) return null;
  const trigger = triggers
    .filter((value) => value.length > 0 && word.startsWith(value))
    .sort((a, b) => b.length - a.length)[0];
  return trigger === undefined
    ? null
    : {
        trigger,
        query: word.slice(trigger.length),
        start: state.selectionEnd - encoder.encode(word).length,
        end: state.selectionEnd,
      };
}

function sameTarget(a: GuiTarget, b: GuiTarget) {
  return (
    a.world.id === b.world.id &&
    a.world.incarnation === b.world.incarnation &&
    a.entity === b.entity &&
    a.component === b.component &&
    a.incarnation === b.incarnation
  );
}

/** One physical context supplies both the caret position and the authorization fence for replacement. */
export function TriggerMenu({
  id: supplied,
  context,
  target,
  triggers,
  candidates,
  onInsert,
  onError,
  width = 220,
}: TriggerMenuProps) {
  const id = useEntityId(supplied);
  const subscribe = useCallback(
    (changed: () => void) => context.onText(changed),
    [context],
  );
  const snapshot = useCallback(() => context.nativeText, [context]);
  const state = useSyncExternalStore(subscribe, snapshot, snapshot);
  const match =
    state && target && sameTarget(state.fence.target, target)
      ? matchTextTrigger(state, triggers)
      : null;
  // Context identities and native fences are unique only within their Host.
  // Caret geometry may change independently while a lookup stays current.
  const request = useMemo(
    () => match && { context, match },
    [
      context,
      target?.world.id,
      target?.world.incarnation,
      target?.entity,
      target?.component,
      target?.incarnation,
      state?.fence.generation,
      match?.trigger,
      match?.query,
      match?.start,
      match?.end,
    ],
  );
  const [dismissed, setDismissed] = useState<typeof request>(null);
  const [result, setResult] = useState<{
    request: NonNullable<typeof request>;
    source: TriggerMenuProps["candidates"];
    options: readonly TriggerOption[];
  } | null>(null);
  const accepted = useRef<typeof request>(null);
  const committed = { context, request, candidates, dismissed, onError };
  const current = useRef<typeof committed | null>(null);
  useLayoutEffect(() => {
    current.current = committed;
    return () => {
      current.current = null;
    };
  });
  useEffect(() => {
    if (!request || request === dismissed) return;
    const controller = new AbortController();
    const isCurrent = () =>
      !controller.signal.aborted &&
      current.current?.request === request &&
      current.current.candidates === candidates &&
      current.current.dismissed !== request;
    const load = async () => {
      try {
        const options = await candidates(request.match, controller.signal);
        if (isCurrent()) setResult({ request, source: candidates, options });
      } catch (reason) {
        if (isCurrent())
          current.current?.onError?.(
            reason instanceof Error ? reason : new Error(String(reason)),
          );
      }
    };
    void load();
    return () => controller.abort();
  }, [request, candidates, dismissed]);
  const options =
    result?.request === request && result?.source === candidates
      ? result.options
      : undefined;
  const caret = state?.caretBounds;
  const shown = !!(
    request &&
    request !== dismissed &&
    options?.length &&
    caret
  );
  const insert = async (key: string) => {
    const live = context.nativeText;
    const option = options?.find((item) => item.key === key && !item.disabled);
    if (
      !request ||
      current.current?.request !== request ||
      current.current.candidates !== candidates ||
      current.current.dismissed === request ||
      accepted.current === request ||
      !shown ||
      !match ||
      !state ||
      !option ||
      !live ||
      !sameTarget(live.fence.target, state.fence.target) ||
      live.fence.generation !== state.fence.generation
    )
      return;
    const check = matchTextTrigger(live, triggers);
    if (
      !check ||
      check.start !== match.start ||
      check.end !== match.end ||
      check.trigger !== match.trigger ||
      check.query !== match.query
    )
      return;
    accepted.current = request;
    setDismissed(request);
    try {
      const ack = await context.editText(live.fence, {
        kind: "replaceRange",
        start: match.start,
        end: match.end,
        text: option.insert,
      });
      if (ack.rejected || ack.cancelled || !ack.applied)
        throw new Error(ack.error ?? "Trigger replacement was not applied");
      if (current.current?.context === context) onInsert?.(option);
    } catch (reason) {
      if (current.current?.context === context)
        onError?.(reason instanceof Error ? reason : new Error(String(reason)));
    }
  };
  return (
    <Resources>
      <Entity id={id}>
        <PresentationGate open />
        <Style x={caret?.[0] ?? 0} y={(caret?.[1] ?? 0) + (caret?.[3] ?? 0)} />
        <Children>
          <Floating
            id={`${id}/popup`}
            mode="light"
            controlled
            open={shown}
            onVisibleChange={(event) => {
              if (
                current.current?.request === request &&
                shown &&
                !event.initial &&
                !event.value
              )
                setDismissed(request);
            }}
            layout={{ width, fit_height: true }}
          >
            {shown && (
              <OptionList
                id={`${id}/options`}
                options={options ?? []}
                onPick={(key) => {
                  void insert(key);
                }}
              />
            )}
          </Floating>
        </Children>
      </Entity>
    </Resources>
  );
}
