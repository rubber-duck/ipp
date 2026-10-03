/** Declaration scopes and session context, independent of a DOM surface. */
import {
  useContext,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { CanvasContext } from "./canvas-context.js";
import {
  notify,
  type CanvasWorldBinding,
  type CanvasWorldSession,
  type IppCanvasHandle,
} from "./canvas-world-session.js";
import type { ReactWorldRoot } from "./index.js";

export interface WorldProps {
  readonly children?: ReactNode;
  readonly onCommit?: (scope: ReactWorldRoot) => void;
  readonly onError?: (error: Error) => void;
}

export function useIppCanvas(): IppCanvasHandle | undefined {
  return useCanvasSession() ?? undefined;
}

function useCanvasSession(): CanvasWorldSession | null {
  const session = useContext(CanvasContext);
  if (session === undefined)
    throw new Error("World and useIppCanvas require a Canvas session");
  return session;
}

export function World({ children, onCommit, onError }: WorldProps) {
  const session = useCanvasSession();
  const binding = useRef<CanvasWorldBinding | undefined>(undefined);
  const callbacks = useRef({ onCommit, onError });
  const [error, setError] = useState<{
    session: CanvasWorldSession;
    error: Error;
  }>();
  useLayoutEffect(() => {
    callbacks.current = { onCommit, onError };
  });
  useLayoutEffect(() => {
    if (!session || session.isClosing) return;
    let active = true;
    const fallback = (error: Error): void => {
      if (active) setError({ session, error });
      else session.report(error);
    };
    const report = (error: Error): void => {
      if (!active) session.report(error);
      else if (callbacks.current.onError)
        notify(() => callbacks.current.onError!(error), fallback);
      else fallback(error);
    };
    const world = session.attach(report);
    binding.current = world;
    return () => {
      active = false;
      binding.current = undefined;
      void world.close().catch(() => {});
    };
  }, [session]);
  useLayoutEffect(() => {
    const world = binding.current;
    if (!session || !world) return;
    world.render(children, (scope) => callbacks.current.onCommit?.(scope));
  }, [children, session]);
  if (error?.session === session) throw error!.error;
  return null;
}
