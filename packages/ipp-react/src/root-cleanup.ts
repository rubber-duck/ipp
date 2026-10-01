export interface RootCleanup {
  retry(): Promise<void>;
  abandon(): Promise<void>;
}

export const rootCleanup = new WeakMap<object, RootCleanup>();
