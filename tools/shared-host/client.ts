/**
 * The contract of a shared-Host client module: a file whose default export
 * is `defineClient({ open, capture, close })`. The shared-host command runs
 * it once (`run`) or keeps it open in a session process (`session`). The
 * module is bundled from source; React, `@ipp/client`, `@ipp/react`,
 * `@ipp/react/gui` and `@ipp/react/gui-kit` resolve to the command's own
 * instances, so modules it loads later render into the same React roots, and
 * `@ipp/host-contract` to the Host's generated client module.
 */
import type {
  Client,
  HostClientBase,
  PresentationView,
  RootBinding,
} from "@ipp/client";
import type { RgbaImage } from "./images.js";

/** A module bundled from source, and the identity of that bundle. */
export interface LoadedModule {
  readonly module: Readonly<Record<string, unknown>>;
  readonly identity: string;
}

export interface ClientContext {
  /** Connection to the running Host through its own generated client. */
  readonly host: HostClientBase<Client>;
  /** The Host's generated client module: descriptors, row encoders, paint keys. */
  readonly contract: Readonly<Record<string, unknown>>;
  /** Checkout the command runs from; relative paths resolve against it. */
  readonly workspace: string;
  /** Run or session name, unique on the Host; use it in World symbolic ids. */
  readonly name: string;
  /** The shared GUI font (`.ippf`) built with the Host. */
  font(): Promise<Uint8Array<ArrayBuffer>>;
  /**
   * Bundle and import a module from source, sharing React and the IPP
   * packages; null while its bundle equals the one with `previous` identity.
   */
  load(path: string, previous?: string): Promise<LoadedModule | null>;
  /**
   * Present `binding` alone on the Host surface while `section` runs: wait
   * for the Host-wide presentation lock, select, run, clear and release.
   */
  present<T>(
    binding: RootBinding,
    section: (view: PresentationView) => Promise<T>,
  ): Promise<T>;
  /** Present `binding` and capture its settled frame. */
  capture(binding: RootBinding): Promise<RgbaImage>;
}

export interface ClientResult {
  /** Images written as `<out>/<key>.png`. */
  readonly images?: Readonly<Record<string, RgbaImage>>;
  /** Structured outcome, returned as JSON with `--json`. */
  readonly report?: unknown;
  /** Lines printed after the written files. */
  readonly summary?: readonly string[];
}

export interface SharedHostClient<State = unknown> {
  /**
   * Create Worlds and declarations. `args` are the command-line arguments
   * after the module (`run`, `session start`).
   */
  open(context: ClientContext, args: readonly string[]): Promise<State>;
  /**
   * Serve one request. `args` are the arguments of `run` or of
   * `session capture`. A session calls the newest bundle of the module
   * with the state its first `open` returned.
   */
  capture(
    state: State,
    context: ClientContext,
    args: readonly string[],
  ): Promise<ClientResult>;
  /** Release what `open` created; temporary Worlds also end with the connection. */
  close(state: State, context: ClientContext): Promise<void>;
}

export function defineClient<State>(
  client: SharedHostClient<State>,
): SharedHostClient<State> {
  return client;
}
