import type {
  AssetWorldClient,
  GuiWorldClient,
  HostClientBase,
  GuiPhysicalContext,
  PresentationView,
  GuiNativeEdit,
} from "@ipp/client";
import { HtmlInputBinding } from "../../../../packages/ipp-client/src/html-input.js";
import { canvasOutput } from "../../../../packages/ipp-client/src/references.js";
import {
  createRoot,
  type ReactWorldClient,
} from "../../../../packages/ipp-react/src/index.js";
import {
  Column,
  GuiKit,
  Textarea,
  OtpInput,
  NumericStepper,
  PasswordInput,
  FileInput,
  TokenInput,
  TOKEN_DOCUMENT_CLIPBOARD_TYPE,
  serializeTokenDocument,
  type GuiFileDescriptor,
  type TokenDocument,
  type GuiFileAdapter,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import { glassSkin } from "../../../../packages/ipp-react/src/gui-glass.js";
import { cybertronSkin } from "../../../../packages/ipp-react/src/gui-cybertron.js";
import { attachCanvasGuiInput } from "../../../../packages/ipp-react/src/gui/platform/input.js";
import { browserGuiFileAdapter } from "../../../../packages/ipp-react/src/gui/platform/file-input.js";
import { assetsLoaded } from "../../support/mounted-panel.js";
import {
  GUI,
  RENDER,
  LIFECYCLE,
  selectSystems,
} from "../../../fixtures/system-selections.js";
import { check } from "../../../harness/page/checks.js";
import type {
  AgentActionsEnvironment,
  AgentActionsContract,
} from "./agent-actions.js";

type Client = GuiWorldClient & AssetWorldClient;

/** A second, independent World mounted on the same native/worker drivers. */
export async function editingControls(
  host: HostClientBase<Client>,
  contract: AgentActionsContract,
  fontBytes: ArrayBuffer,
  environment: AgentActionsEnvironment,
) {
  const world = await host.createWorld({
    symbolicId: "editing-specimen",
    temporary: true,
    selectedSystems: selectSystems(GUI, RENDER, LIFECYCLE),
    canvas: { extent: [320, 400], unitsPerMetre: 320 },
  });
  const client = await host.openWorld(world.reference);
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (error) => errors.push(error),
  });
  let view: PresentationView | undefined,
    context: GuiPhysicalContext | undefined,
    detach: (() => void) | undefined;
  const form = document.createElement("form"),
    canvas = document.createElement("canvas");
  form.id = "editing-specimen-form";
  document.body.append(form, canvas);
  try {
    const font = await client.createAsset(17, fontBytes.slice(0));
    const agent = contract.createGuiAgent({ main: client });
    const reports = {
      otp: [] as string[],
      tokens: [] as TokenDocument[],
      files: [] as (readonly GuiFileDescriptor[])[],
      rejects: [] as string[],
      submits: [] as string[],
      selection: [] as (readonly [number, number])[],
    };
    const selected: GuiFileDescriptor = {
      id: "text",
      name: "notes.txt",
      size: 2,
      type: "text/plain",
    };
    let choose: GuiFileAdapter<GuiFileDescriptor>["choose"] = async () => [
      { ...selected, id: "image", name: "bad.png", type: "image/png" },
      selected,
    ];
    let fileLifetime = 0;
    let maxFiles = 1;
    let fileValue: readonly GuiFileDescriptor[] | undefined;
    let otpMasked = true;
    let controlledCode: string | undefined;
    let passwordPurpose: "current-password" | "new-password" =
      "current-password";
    let controlledTokens: TokenDocument | undefined;
    let controlledMemo: string | undefined;
    const adapter: GuiFileAdapter<GuiFileDescriptor> = {
      capabilities: { choose: true, drop: false, paste: false },
      choose: (request) => choose!(request),
    };
    const tokens: TokenDocument = [
      { kind: "text", text: "a" },
      { kind: "token", id: "fixture", label: "Mars" },
      { kind: "text", text: "é" },
    ];
    async function waitFor(checkState: () => boolean, description: string) {
      const until = performance.now() + 12000;
      while (!checkState() && performance.now() < until) {
        await client.waitForFrame();
        await root.flush();
      }
      check(checkState(), description);
    }
    const draw = async (skin: ReturnType<typeof cybertronSkin>) => {
      await root.render(
        <GuiKit
          contract={contract}
          skin={skin}
          font={font.source}
          fontSize={11}
          reducedMotion
        >
          <Column id="editing" width={320} height={400} padding={8} gap={4}>
            <Textarea
              id="memo"
              label="Memo"
              rows={2}
              defaultValue="a"
              {...(controlledMemo === undefined
                ? {}
                : { value: controlledMemo })}
              required
              description="Field notes"
              error="Review entry"
              onSubmit={(event) => reports.submits.push(event.value)}
            />
            <PasswordInput
              id="password"
              label="Password"
              defaultValue="Véctor"
              purpose={passwordPurpose}
            />
            <OtpInput
              id="otp"
              label="Code"
              length={4}
              masked={otpMasked}
              {...(controlledCode === undefined
                ? {}
                : { value: controlledCode })}
              onComplete={(code) => reports.otp.push(code)}
            />
            <NumericStepper
              id="fixed-number"
              label="Fixed number"
              defaultValue={7.25}
              precision={0}
              readOnly
            />
            <TokenInput
              id="tokens"
              label="Tokens"
              defaultValue={tokens}
              {...(controlledTokens === undefined
                ? {}
                : { value: controlledTokens })}
              clipboard={{
                write: async () => {},
                acceptsToken: (token) => token.id === "empty",
              }}
              onSelectionChange={(selection) =>
                reports.selection.push(selection)
              }
              onChange={(document) => reports.tokens.push(document)}
            />
            <FileInput
              key={fileLifetime}
              id={`files-${fileLifetime}`}
              label="Choose attachment"
              accept={["text/plain"]}
              adapter={adapter}
              maxFiles={maxFiles}
              {...(fileValue ? { value: fileValue } : {})}
              onChange={(files) => reports.files.push(files)}
              onReject={(_, reason) => reports.rejects.push(reason)}
            />
          </Column>
        </GuiKit>,
      );
      await root.flush();
      await client.waitForFrame();
    };
    await draw(cybertronSkin(contract));
    await assetsLoaded(client, [font.source]);
    const output = canvasOutput(world.reference);
    const binding = await host.setRootOutput(output, {
      width: 320,
      height: 400,
      devicePixelRatio: 1,
    });
    view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    const input = await host.input.open(view);
    context = input;
    detach = attachCanvasGuiInput(canvas, input, {
      onError: (error) => errors.push(error),
      textForm: () => ({ form: form.id, name: "focused" }),
    });
    async function act(label: string, kind: "focus" | "press" = "focus") {
      const result = await agent.act({
        steps: [{ target: { label }, action: { kind } }],
      });
      check(
        result.ok,
        `Editing action ${kind} ${label}: ${JSON.stringify(result.steps)}`,
      );
      await root.flush();
      await client.waitForFrame();
    }
    async function edit(command: GuiNativeEdit) {
      const state = input.nativeText;
      check(state, "Focused native text");
      const result = await input.editText(state.fence, command);
      check(
        !result.rejected && !result.cancelled && !result.error,
        `Native edit ${command.kind}: ${result.error}`,
      );
      await client.waitForFrame();
      await root.flush();
    }
    async function shot(name: string) {
      const image = await host.presentation.capture(view!, {
        afterOutputs: [output],
      });
      check(
        image.drawCalls > 0 && !image.failedDrawCalls,
        "Completed editor draws",
      );
      const pixels = new Uint8Array(image.pixels);
      let colored = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        if (
          Math.max(pixels[i]!, pixels[i + 1]!, pixels[i + 2]!) -
            Math.min(pixels[i]!, pixels[i + 1]!, pixels[i + 2]!) >
          20
        )
          colored++;
      }
      check(
        colored > 100,
        `Editor paints themed controls (${colored} chromatic pixels)`,
      );
      await environment.capture(name, image);
      return pixels;
    }
    const memo = (await agent.query({ label: "Memo" })).controls[0]!;
    check(
      memo.semantics?.description === "Field notes" &&
        memo.semantics.error === "Review entry" &&
        memo.semantics.required &&
        memo.semantics.invalid,
      "The label and validation details belong to Memo",
    );
    await act("Focus Memo", "press");
    await waitFor(
      () => input.nativeText?.multiline === true,
      "Label activates the multiline editing target",
    );
    await input.send({ kind: "key", key: "enter" });
    await waitFor(
      () => input.nativeText?.text === "a\n",
      "Physical Enter inserts LF",
    );
    await edit({ kind: "text", text: "éabc" });
    await edit({ kind: "navigate", direction: "home", extend: true });
    check(
      input.nativeText?.selectionStart === 7 &&
        input.nativeText.selectionEnd === 2,
      "Visual-line Home extends the native UTF-8 selection",
    );
    await edit({ kind: "key", key: "enter" });
    await waitFor(
      () => reports.submits.at(-1) === "a\néabc",
      "Explicit submit retains the newline",
    );
    const memoFence = input.nativeText!.fence;
    controlledMemo = "owner";
    await draw(cybertronSkin(contract));
    await waitFor(
      () => input.nativeText?.text === "owner",
      "A controlled replacement updates the focused textarea",
    );
    check(
      input.nativeText!.fence.target.entity === memoFence.target.entity &&
        input.nativeText!.fence.generation !== memoFence.generation,
      "External replacement preserves the textarea target and refreshes the edit generation",
    );
    const late = await input.editText(memoFence, {
      kind: "text",
      text: "late",
    });
    check(
      !!(late.error || late.rejected || late.cancelled) &&
        input.nativeText!.text === "owner",
      "Old textarea work cannot reach the new editing generation",
    );
    await act("Password");
    await waitFor(
      () => input.nativeText?.purpose === "current-password",
      "Password purpose on the exact native focus",
    );
    const area = document.querySelector<HTMLTextAreaElement>(
      `[data-ipp-native-text][form="${form.id}"]`,
    );
    check(
      area?.autocomplete === "current-password" &&
        new FormData(form).get("focused") === "Véctor",
      "Focused form receives the fixture and password autocomplete hint",
    );
    await edit({ kind: "selection", start: 1, end: 3 });
    const password = input.nativeText!;
    const masked = await shot("editing-cybertron-masked");
    await act("Show password", "press");
    await waitFor(
      () => input.nativeText?.masked === false,
      "Password reveal reaches the same input",
    );
    check(
      password.fence.generation === input.nativeText!.fence.generation &&
        Number(input.nativeText!.selectionEnd) === 3,
      "Reveal preserves selection and editing generation",
    );
    const revealed = await shot("editing-cybertron-revealed");
    check(
      masked.some((n, i) => n !== revealed[i]),
      "Reveal changes painted pixels",
    );
    check(
      (await agent.query({ label: "Password" })).controls[0]?.state.text ===
        null,
      "Agent snapshots redact even revealed passwords",
    );
    const revealedFence = input.nativeText!.fence;
    passwordPurpose = "new-password";
    await draw(cybertronSkin(contract));
    await waitFor(
      () => input.nativeText?.purpose === "new-password",
      "A purpose-only update reaches an already-revealed focused editor",
    );
    check(
      input.nativeText!.fence.target.entity === revealedFence.target.entity &&
        input.nativeText!.fence.generation !== revealedFence.generation &&
        !input.nativeText!.masked,
      "Changing autocomplete purpose preserves the target and renews its editing fence",
    );
    const htmlTarget = (await agent.query({ label: "Tokens" })).controls[0]!;
    const content = document.createElement("div");
    content.append(document.createElement("input"));
    document.body.append(content);
    const html = new HtmlInputBinding(canvas, content, {
      world: world.reference,
      entity: BigInt(htmlTarget.entity),
      component: htmlTarget.component,
      incarnation: BigInt(htmlTarget.incarnation),
    });
    try {
      await act("Tokens");
      await waitFor(
        () => document.activeElement === content,
        "The registered HTML editor receives focus",
      );
      check(
        !area.value &&
          !area.hasAttribute("form") &&
          !area.hasAttribute("name") &&
          String(area.autocomplete) === "off" &&
          !new FormData(form).has("focused"),
        "Direct HTML focus handoff drops the old password, autocomplete and form association",
      );
    } finally {
      html.release();
      content.remove();
    }
    await act("Code");
    await waitFor(
      () => input.nativeText?.purpose === "one-time-code",
      "OTP owns one native focus",
    );
    await edit({ kind: "text", text: "12x345" });
    await waitFor(
      () => reports.otp.join(",") === "1234",
      "OTP filters the pasted code and commits completion once",
    );
    check(
      (await agent.query({ label: "Code" })).controls[0]?.state.text === null,
      "The agent redacts one-time codes while masked",
    );
    async function readSlotText() {
      let after = 0n;
      const slots: string[] = [];
      do {
        const page = await client.inspectPage({
          collection: "entities",
          after,
          limit: 128,
        });
        for (const entity of page.entities)
          if (entity.metadata.symbolicId?.startsWith("otp/slot/")) {
            const visible = entity.components.find(
              (c) => c.component === client.components.CanvasText?.id,
            )?.fields.text;
            if (typeof visible === "string") slots.push(visible);
          }
        after = page.next;
      } while (after);
      return slots;
    }
    check(
      (await readSlotText()).join("") === "••••",
      "All populated masked slots render a bullet",
    );
    otpMasked = false;
    await draw(cybertronSkin(contract));
    check(
      (await agent.query({ label: "Code" })).controls[0]?.state.text === null,
      "The semantic one-time code stays redacted with ordinary visual slots",
    );
    controlledCode = "123";
    await draw(cybertronSkin(contract));
    await waitFor(
      () => input.nativeText?.text === "123",
      "Controlled OTP accepts its external incomplete value",
    );
    for (const [length, digit] of [
      [2, "4"],
      [3, "5"],
    ] as const) {
      await edit({ kind: "text", text: digit });
      await waitFor(
        () => reports.otp.length === length && input.nativeText?.text === "123",
        "Rejected completion rolls back to three digits",
      );
    }
    check(
      reports.otp.join(",") === "1234,1234,1235",
      "Each rejected completion can cross the incomplete boundary again",
    );
    await act("Fixed number");
    await waitFor(
      () => !!input.nativeText?.readOnly,
      "The physical numeric record carries its read-only policy",
    );
    await edit({ kind: "key", key: "enter" });
    check(
      input.nativeText!.readOnly,
      "Submitting a rounded numeric display preserves read-only",
    );
    const number = (await agent.query({ label: "Fixed number" })).controls[0]!;
    const target = input.nativeText!.fence.target;
    const result = await client.batch([
      {
        kind: "guiAction",
        entity: { kind: "handle", id: target.entity },
        component: target.component,
        incarnation: target.incarnation,
        action: { kind: "scalar", value: 12 },
      },
    ]);
    check(
      !result.ok && result.error.reason === "Unavailable",
      "Core refuses a direct scalar write to a read-only numeric input",
    );
    const numPage = await client.inspectPage({
      collection: "entities",
      target: BigInt(number.entity),
    });
    const numBox = numPage.entities[0]!.components.find(
      (c) => c.component === client.components.CanvasBounds!.id,
    )!.fields;
    const point: [number, number] = [
      (Number(numBox.x) + Number(numBox.width) * 0.98) / 320,
      (Number(numBox.y) + Number(numBox.height) * 0.5) / 400,
    ];
    await input.send({ kind: "key", key: "up" });
    await input.send({ kind: "wheel", point, delta: [0, -1] });
    await input.send({ kind: "pointerDown", pointer: 72n, point });
    await input.send({ kind: "pointerUp", pointer: 72n, point });
    const observed = (await agent.query({ label: "Fixed number" }))
      .controls[0]!;
    check(
      observed.state.value === 7.25 &&
        observed.actions.includes("submit") &&
        !observed.actions.includes("scalar"),
      "Submission, wheel, step and pointer input preserve the unrounded read-only value",
    );
    await act("Tokens");
    await waitFor(
      () => !!input.nativeText?.clientInput,
      "Application token editor owns the routed input target",
    );
    const tokenRow = (await agent.query({ label: "Tokens" })).controls[0]!;
    const tokenEntity = (
      await client.inspectPage({
        collection: "entities",
        target: BigInt(tokenRow.entity),
      })
    ).entities[0]!;
    const box = tokenEntity.components.find(
      (c) => c.component === client.components.CanvasBounds!.id,
    )!.fields;
    const pointer = (
      kind: "pointerDown" | "pointerMove" | "pointerUp",
      x: number,
    ) =>
      input.send({
        kind,
        pointer: 711n,
        point: [
          (Number(box.x) + x) / 320,
          (Number(box.y) + Number(box.height) * 0.5) / 400,
        ],
      });
    await pointer("pointerDown", 0.2);
    await pointer("pointerMove", Number(box.width) - 1);
    await pointer("pointerUp", Number(box.width) - 1);
    await waitFor(
      () => JSON.stringify(reports.selection.at(-1)) === "[0,3]",
      "Captured pointer drag selects all three measured units including the whole token",
    );
    for (const key of ["End", "ArrowLeft", "Backspace"]) {
      const result = await input.send({
        kind: "clientInput",
        fence: input.nativeText!.fence,
        input: {
          kind: "key",
          key,
          code: key,
          modifiers: 0,
          pressed: true,
          repeat: false,
        },
      });
      check(!result.error && !result.rejected, "Token key accepted");
    }
    await waitFor(
      () =>
        JSON.stringify(reports.tokens.at(-1)) ===
        JSON.stringify([{ kind: "text", text: "aé" }]),
      "Backspace removes one whole token",
    );
    controlledTokens = [{ kind: "text", text: "next" }];
    const former = input.nativeText!.fence;
    await draw(cybertronSkin(contract));
    await act("Tokens");
    await waitFor(
      () =>
        !!input.nativeText?.clientInput &&
        input.nativeText.fence.target.entity !== former.target.entity,
      "External token replacement creates a fresh physical target",
    );
    const stale = await input.send({
      kind: "clientInput",
      fence: former,
      input: { kind: "text", text: "late" },
    });
    check(
      !!(stale.rejected || stale.cancelled || stale.error),
      "Stale input cannot modify the new document",
    );
    const current = input.nativeText!.fence;
    const wrote = () =>
      input.send({
        kind: "clientInput",
        fence: current,
        input: { kind: "text", text: "!" },
      });
    await wrote();
    await waitFor(
      () => reports.tokens.length === 2,
      "Controlled edit proposal delivered",
    );
    const refused = JSON.stringify(reports.tokens.at(-1));
    await wrote();
    await waitFor(
      () => reports.tokens.length === 3,
      "Second proposal delivered after refusal",
    );
    check(
      JSON.stringify(reports.tokens.at(-1)) === refused,
      "A rejected controlled edit leaves selection and canonical document intact",
    );
    const emptyToken = { kind: "token", id: "empty", label: "" } as const;
    const copied = serializeTokenDocument([emptyToken]);
    const transfer = new DataTransfer();
    transfer.setData("text/plain", copied.text);
    transfer.setData(TOKEN_DOCUMENT_CLIPBOARD_TYPE, copied.structured!);
    area.dispatchEvent(
      new ClipboardEvent("paste", {
        clipboardData: transfer,
        cancelable: true,
      }),
    );
    await waitFor(
      () => reports.tokens.length === 4,
      "A browser paste delivers structured data with an empty projection",
    );
    check(
      reports.tokens
        .at(-1)
        ?.some((part) => part.kind === "token" && part.id === "empty") === true,
      "The original empty-label token survives an authorized clipboard gesture",
    );
    await act("Choose attachment", "press");
    await waitFor(
      () =>
        reports.files.at(-1)?.[0]?.id === "text" &&
        reports.rejects[0] === "type",
      "Only valid file descriptors reach the application",
    );
    await act("Remove notes.txt", "press");
    await waitFor(
      () => reports.files.at(-1)?.length === 0,
      "Removal publishes the empty file list",
    );
    const beforeCancel = reports.files.length;
    choose = async () => null;
    await act("Choose attachment", "press");
    check(
      reports.files.length === beforeCancel,
      "File chooser cancellation preserves the selection",
    );
    let release: ((files: readonly GuiFileDescriptor[]) => void) | undefined;
    choose = () =>
      new Promise((resolve) => {
        release = resolve;
      });
    await act("Choose attachment", "press");
    await waitFor(() => !!release, "A file request is outstanding");
    fileLifetime++;
    await draw(glassSkin(contract));
    release!([selected]);
    await root.flush();
    await client.waitForFrame();
    check(
      reports.files.length === beforeCancel,
      "A late chooser result cannot update a replacement",
    );
    // A controlled list can retain descriptors after its allowed count shrinks.
    maxFiles = 2;
    fileValue = [
      selected,
      { ...selected, id: "b", name: "second.txt" },
      { ...selected, id: "c", name: "third.txt" },
    ];
    choose = async () => [
      { ...selected, id: "overflow", name: "overflow.txt" },
    ];
    await draw(glassSkin(contract));
    await act("Choose attachment", "press");
    await waitFor(
      () => reports.rejects.at(-1) === "count",
      "No file is admitted while the existing list exceeds its new bound",
    );
    check(
      reports.files.length === beforeCancel,
      "A rejected file emits no accepted list",
    );
    await shot("editing-glass");
    check(
      errors.length === 0,
      `Editor errors: ${errors.map((e) => e.message).join(",")}`,
    );
    await verifyBrowserFiles(environment);
  } finally {
    detach?.();
    await context?.close();
    if (view) await host.presentation.clear(view);
    form.remove();
    canvas.remove();
    await root.unmount();
    await host.destroyWorld(world.reference);
  }
}

async function verifyBrowserFiles(environment: AgentActionsEnvironment) {
  const surface = document.createElement("div"),
    picker = document.createElement("input");
  picker.type = "file";
  picker.id = "gui-editor-native-picker";
  document.body.append(picker);
  const files: File[] = [];
  const adapter = browserGuiFileAdapter({ surface, picker });
  const controller = new AbortController();
  const dispose = adapter.subscribe!(
    { signal: controller.signal, accept: ["text/plain"], multiple: true },
    (descriptors) => files.push(...descriptors.map((d) => d.file)),
  );
  const transfer = new DataTransfer();
  transfer.items.add(
    new File(["fixture"], "notes.txt", { type: "text/plain" }),
  );
  await environment.chooseBrowserFile("#gui-editor-native-picker");
  check(files.length === 1, "Native browser chooser delivers a File");
  surface.dispatchEvent(
    new DragEvent("drop", { dataTransfer: transfer, cancelable: true }),
  );
  surface.dispatchEvent(
    new ClipboardEvent("paste", { clipboardData: transfer, cancelable: true }),
  );
  check(
    Number(files.length) === 3 &&
      files.every((f) => f.name === "notes.txt" && f.size === 7),
    "Browser picker, drop and paste provide application-owned File descriptors",
  );
  controller.abort();
  dispose?.();
  picker.remove();
  surface.dispatchEvent(new DragEvent("drop", { dataTransfer: transfer }));
  check(
    Number(files.length) === 3,
    "A retired browser receiver cannot deliver files",
  );
}
