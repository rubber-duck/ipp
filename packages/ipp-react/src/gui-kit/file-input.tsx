import { useLayoutEffect, useRef, useState } from "react";
import { useEntityId } from "../scope/identity.js";
import { Column, Row, type GuiKitLayoutProps } from "./layout.js";
import { TextLine } from "./text.js";
import { SecondaryButton } from "./secondary-button.js";

export interface GuiFileDescriptor {
  readonly id: string;
  readonly name: string;
  readonly size: number;
  readonly type: string;
}
export interface GuiFileRequest {
  readonly accept: readonly string[];
  readonly multiple: boolean;
  readonly signal: AbortSignal;
}
export interface GuiFileAdapter<FileType extends GuiFileDescriptor> {
  readonly capabilities: Readonly<{
    choose: boolean;
    drop: boolean;
    paste: boolean;
  }>;
  /** Null means cancel. Return application-owned descriptors; the GUI never owns file bytes. */
  choose?(request: GuiFileRequest): Promise<readonly FileType[] | null>;
  subscribe?(
    request: GuiFileRequest,
    receive: (files: readonly FileType[]) => void,
  ): void | (() => void);
}
export type GuiFileRejection =
  | "metadata"
  | "type"
  | "size"
  | "count"
  | "duplicate";

export function validateGuiFile(
  file: GuiFileDescriptor,
  accept: readonly string[],
  maxBytes: number,
): GuiFileRejection | undefined {
  if (
    !file.id ||
    !file.name ||
    typeof file.type !== "string" ||
    !Number.isSafeInteger(file.size) ||
    file.size < 0
  )
    return "metadata";
  if (file.size > maxBytes) return "size";
  const name = file.name.toLowerCase(),
    type = file.type.toLowerCase();
  if (
    accept.length &&
    !accept.some((rule) => {
      const pattern = rule.trim().toLowerCase();
      if (pattern.startsWith(".")) return name.endsWith(pattern);
      if (pattern.endsWith("/*")) return type.startsWith(pattern.slice(0, -1));
      return pattern === type;
    })
  )
    return "type";
  return undefined;
}

export interface FileInputProps<F extends GuiFileDescriptor = GuiFileDescriptor>
  extends GuiKitLayoutProps {
  readonly id?: string;
  readonly label?: string;
  readonly adapter?: GuiFileAdapter<F>;
  readonly accept?: readonly string[];
  readonly maxFiles?: number;
  readonly maxBytes?: number;
  readonly disabled?: boolean;
  readonly value?: readonly F[];
  readonly defaultValue?: readonly F[];
  readonly onChange?: (files: readonly F[]) => void;
  readonly onReject?: (file: F, reason: GuiFileRejection) => void;
  readonly onError?: (error: unknown) => void;
}
const noFiles: readonly never[] = [];
export function FileInput<F extends GuiFileDescriptor>({
  id: provided,
  label = "Choose files",
  adapter,
  accept = noFiles,
  maxFiles = 1,
  maxBytes = Number.MAX_SAFE_INTEGER,
  disabled = false,
  value,
  defaultValue = noFiles,
  onChange,
  onReject,
  onError,
  ...layout
}: FileInputProps<F>) {
  if (
    !Number.isSafeInteger(maxFiles) ||
    maxFiles < 1 ||
    !Number.isSafeInteger(maxBytes) ||
    maxBytes < 0
  )
    throw new RangeError("Invalid file count or byte limit");
  const id = useEntityId(provided);
  const [own, setOwn] = useState(defaultValue);
  const current = value ?? own;
  const model = useRef({
    current,
    value,
    maxFiles,
    maxBytes,
    onChange,
    onReject,
    onError,
    accept,
  });
  useLayoutEffect(() => {
    model.current = {
      current,
      value,
      maxFiles,
      maxBytes,
      onChange,
      onReject,
      onError,
      accept,
    };
  });
  const [problem, setProblem] = useState("");
  const holder = useRef<AbortController | null>(null);
  const pending = useRef<AbortController | null>(null);
  const acceptKey = JSON.stringify(accept);
  function change(next: readonly F[]) {
    if (model.current.value === undefined) {
      model.current.current = next;
      setOwn(next);
    }
    model.current.onChange?.(next);
  }
  function receive(files: readonly F[]) {
    const m = model.current;
    let next = m.maxFiles === 1 ? [] : [...m.current];
    let added = 0;
    const failures: string[] = [];
    for (const file of files) {
      const reason =
        validateGuiFile(file, m.accept, m.maxBytes) ??
        (next.some((prior) => prior.id === file.id)
          ? "duplicate"
          : next.length >= m.maxFiles
            ? "count"
            : undefined);
      if (reason) {
        failures.push(`${file.name}: ${reason}`);
        m.onReject?.(file, reason);
        continue;
      }
      next = [...next, file];
      added++;
    }
    setProblem(failures.join(", "));
    if (
      added &&
      (next.length !== m.current.length ||
        next.some((file, i) => file !== m.current[i]))
    )
      change(next);
  }
  useLayoutEffect(() => {
    if (!adapter || disabled) return;
    const lease = new AbortController();
    holder.current = lease;
    const dispose = adapter.subscribe?.(
      {
        accept: JSON.parse(acceptKey),
        multiple: maxFiles > 1,
        signal: lease.signal,
      },
      (files) => {
        if (!lease.signal.aborted) receive(files);
      },
    );
    return () => {
      lease.abort();
      pending.current?.abort();
      pending.current = null;
      holder.current = null;
      dispose?.();
    };
  }, [id, adapter, acceptKey, maxFiles, disabled]);
  function choose() {
    if (!adapter?.choose || !holder.current || disabled) return;
    pending.current?.abort();
    const request = new AbortController();
    pending.current = request;
    void Promise.resolve()
      .then(() => {
        if (request.signal.aborted) return null;
        return adapter.choose!({
          accept: model.current.accept,
          multiple: maxFiles > 1,
          signal: request.signal,
        });
      })
      .then((files) => {
        if (files && !request.signal.aborted) receive(files);
      })
      .catch((error: unknown) => {
        if (request.signal.aborted) return;
        setProblem(
          error instanceof Error ? error.message : "File selection failed",
        );
        model.current.onError?.(error);
      });
  }
  const cap = adapter?.capabilities;
  return (
    <Column {...layout} id={id}>
      <SecondaryButton
        id={`${id}/choose`}
        label={label}
        disabled={disabled || !cap?.choose || !adapter?.choose}
        onPress={choose}
      />
      {!cap?.choose && (
        <TextLine text="File chooser unavailable" tone="neutral" />
      )}
      {cap?.drop && <TextLine text="Drop files here" tone="neutral" />}
      {cap?.paste && <TextLine text="Paste files here" tone="neutral" />}
      {current.map((file) => (
        <Row key={file.id} fitHeight>
          <TextLine text={file.name} />
          <SecondaryButton
            label={`Remove ${file.name}`}
            disabled={disabled}
            onPress={() =>
              change(
                model.current.current.filter((entry) => entry.id !== file.id),
              )
            }
          />
        </Row>
      ))}
      {current.length > 0 && (
        <SecondaryButton
          id={`${id}/clear`}
          label="Clear files"
          disabled={disabled}
          onPress={() => change([])}
        />
      )}
      {problem && <TextLine text={problem} tone="error" />}
    </Column>
  );
}
