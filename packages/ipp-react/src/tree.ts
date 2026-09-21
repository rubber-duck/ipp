import {
  ANIMATION_HOST_TYPE,
  type AnimationDescription,
  type AnimationProps,
  type AnimationMailbox,
} from "./animation.js";
import { describeAnimation, animationSignature } from "./animation_tree.js";
import { SurfaceItemDeclarations } from "./surface_items.js";
import type { SurfaceItemProps } from "./components.js";
import {
  isAsset,
  ANIMATION_ASSET_HOST_TYPE,
  ASSET_HOST_TYPE,
  SHADER_ASSET_HOST_TYPE,
  isAssetReference,
  byteSignature,
  type AssetHostType,
  type AssetDescription,
  type AssetProps,
  type ShaderAssetProps,
} from "./assets.js";
import {
  isShader,
  shaderProps,
  describeShader,
  type ShaderHostType,
} from "./shaders.js";
import type {
  ComponentOverlayMode,
  EntityOverlayMode,
  ReactWorldClient,
} from "./contract.js";
import {
  FieldKind,
  inferDynamicValue,
  type DynamicValue,
  encodeShaderDefinition,
  type ComponentDescriptor,
  type FieldKind as FieldKindValue,
  type FieldValue,
} from "@ipp/client";
import {
  componentNames,
  ENTITY_HOST_TYPE,
  CHILDREN_HOST_TYPE,
  componentContract,
  type ReactWorldComponentType,
} from "./components.js";
import {
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_IMAGE_HOST_TYPE,
  GUI_ROOT_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  guiContentFor,
  guiStyleFor,
  isGuiHostType,
  isGuiLeafHostType,
  validateGuiNodeRef,
  type GuiActionListener,
  type GuiHostType,
  type GuiNodeProps,
  type GuiNodeRef,
} from "./gui/components.js";
import {
  guiRootSignature,
  type GuiDescribedNode,
  type GuiDescribedRoot,
} from "./gui/description.js";
import type {
  GuiPressListener,
  GuiScalarCommitListener,
  GuiTextCommitListener,
  GuiToggleListener,
} from "./gui/callbacks.js";
import { guiStyleWithTheme, validateGuiTheme } from "./gui/theme.js";
import {
  validateButtonProps,
  validateCheckboxProps,
  validateSliderProps,
  validateTextInputProps,
  type ButtonProps,
  type CheckboxProps,
  type SliderProps,
  type TextInputProps,
} from "./gui/controls.js";

export type ReactWorldElementType =
  | typeof ANIMATION_HOST_TYPE
  | typeof CHILDREN_HOST_TYPE
  | typeof ENTITY_HOST_TYPE
  | ReactWorldComponentType
  | ShaderHostType
  | AssetHostType
  | GuiHostType;
export type StateOverlayFieldValue = Extract<
  FieldValue,
  { kind: "f32" | "u32" | "u64" | "string" | "bytes" | "bool" | "entity" }
>;
/** A local declaration dependency, resolved to a runtime entity after attachment. */
export type ReactWorldFieldValue =
  | StateOverlayFieldValue
  | { kind: "binding"; value: number }
  | { kind: "asset"; value: string };
export type ReactWorldElementProps = Readonly<Record<string, unknown>>;

const declarationFieldKinds: ReadonlySet<FieldKindValue> = new Set([
  FieldKind.F32,
  FieldKind.U32,
  FieldKind.U64,
  FieldKind.Entity,
  FieldKind.String,
  FieldKind.Bytes,
  FieldKind.Bool,
]);

export interface ReactWorldInstance {
  readonly identity: number;
  readonly type: ReactWorldElementType;
  props: ReactWorldElementProps;
  children: ReactWorldInstance[];
  hidden: boolean;
}

export interface EntityOverlayBindingDescription {
  readonly identity: number;
  readonly symbolicId: string;
  readonly mode: EntityOverlayMode;
}

export interface ComponentStateOverlayDescription {
  readonly identity: number;
  readonly entity: number;
  readonly component: number;
  readonly mode: ComponentOverlayMode;
  readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  readonly properties?: Readonly<Record<string, DynamicValue>>;
}

export interface ReactWorldDescription {
  readonly animations: readonly AnimationDescription[];
  readonly assets: readonly AssetDescription[];
  readonly entities: readonly EntityOverlayBindingDescription[];
  readonly overlays: readonly ComponentStateOverlayDescription[];
  readonly gui: readonly GuiDescribedRoot[];
  readonly signature: string;
}

/** JS-only listeners retained on one described GUI node. The commit signature
 * and diff ignore them, so changing a listener alone resubmits nothing. */
export interface GuiRetainedNodeCallbacks {
  readonly onAction: GuiActionListener | undefined;
  readonly onActionCapture: GuiActionListener | undefined;
  readonly onPress: GuiPressListener | undefined;
  readonly onToggle: GuiToggleListener | undefined;
  readonly onScalarCommit: GuiScalarCommitListener | undefined;
  readonly onTextCommit: GuiTextCommitListener | undefined;
}

/** Described GUI node with its JS-only callback seam attached. This remains
 * assignable to `GuiDescribedNode`, so descriptions keep flowing through the
 * existing commit types without transport changes. */
export interface GuiDescribedNodeWithCallbacks extends GuiDescribedNode {
  readonly callbackSeam: GuiRetainedNodeCallbacks;
}

/** Read the JS-only listeners retained on a described GUI node. Nodes built
 * outside `ReactWorldTree.describe` carry no seam and report no listeners. */
export function retainedNodeCallbacks(
  node: GuiDescribedNode,
): GuiRetainedNodeCallbacks {
  const seam = (node as Partial<GuiDescribedNodeWithCallbacks>).callbackSeam;
  return (
    seam ?? {
      onAction: undefined,
      onActionCapture: undefined,
      onPress: undefined,
      onToggle: undefined,
      onScalarCommit: undefined,
      onTextCommit: undefined,
    }
  );
}

export class ReactWorldTree {
  private readonly surfaceItems = new WeakMap<
    ReactWorldInstance,
    SurfaceItemDeclarations
  >();
  children: ReactWorldInstance[] = [];
  private readonly components: ReactWorldClient["components"];
  private nextIdentity = 1;
  private nextAssetVersion = 1;
  private readonly byteSignatures = new WeakMap<
    Uint8Array<ArrayBuffer>,
    string
  >();
  private readonly surfaceBytes = new WeakSet<Uint8Array<ArrayBuffer>>();
  private readonly assetEncodings = new WeakMap<
    ReactWorldInstance,
    {
      inputs: readonly unknown[];
      bytes: Uint8Array<ArrayBuffer>;
      signature: string;
      version: number;
    }
  >();

  private encodedAsset(
    instance: ReactWorldInstance,
    inputs: readonly unknown[],
    encode: () => Uint8Array<ArrayBuffer>,
  ) {
    const previous = this.assetEncodings.get(instance);
    if (
      previous &&
      inputs.length === previous.inputs.length &&
      inputs.every((value, index) => Object.is(value, previous.inputs[index]))
    )
      return previous;
    const encoded = encode();
    if (!(encoded instanceof Uint8Array))
      throw new Error("Asset encoder must return Uint8Array synchronously");
    const bytes = encoded.slice();
    const signature = byteSignature(bytes);
    const result = {
      inputs,
      bytes,
      signature,
      version:
        signature === previous?.signature
          ? previous.version
          : this.nextAssetVersion++,
    };
    this.assetEncodings.set(instance, result);
    return result;
  }

  constructor(private readonly client: ReactWorldClient) {
    this.components = client.components;
  }

  private byteSignature(bytes: Uint8Array<ArrayBuffer>): string {
    if (!this.surfaceBytes.has(bytes)) return byteSignature(bytes);
    let signature = this.byteSignatures.get(bytes);
    if (signature === undefined) {
      signature = byteSignature(bytes);
      this.byteSignatures.set(bytes, signature);
    }
    return signature;
  }

  validate(type: ReactWorldElementType, props: ReactWorldElementProps): void {
    if (isGuiHostType(type)) {
      this.validateGui(type, props);
      return;
    }
    if (props.ref != null) throw new Error("Remote refs are not supported yet");
    if (type === ANIMATION_HOST_TYPE) {
      if (
        !this.client.createAnimationController ||
        !this.client.updateAnimationController ||
        !this.client.deleteAnimationController ||
        !this.client.controlAnimationController ||
        !this.client.onPlaybackEvent
      )
        throw new Error("Animation requires an animation-capable client");
      if (
        props.onPlaybackEvent !== undefined &&
        typeof props.onPlaybackEvent !== "function"
      )
        throw new Error("Animation onPlaybackEvent must be a function");
      for (const key of Object.keys(props))
        if (
          ![
            "source",
            "target",
            "bindings",
            "speed",
            "looping",
            "transition",
            "autoPlay",
            "onPlaybackEvent",
            "mailbox",
          ].includes(key)
        )
          throw new Error(`Unsupported Animation prop: ${key}`);
      return;
    }
    if (isAsset(type)) {
      if (typeof props.id !== "string" || !props.id)
        throw new Error("Asset requires a nonempty id");
      if (
        type === ASSET_HOST_TYPE &&
        (typeof props.encode !== "function" ||
          !Number.isInteger(props.kind) ||
          (props.kind as number) <= 0 ||
          (props.kind as number) > 65535)
      )
        throw new Error("Asset requires a kind and synchronous encoder");
      if (
        type === SHADER_ASSET_HOST_TYPE &&
        (!props.recipe || !props.parameters)
      )
        throw new Error(
          "ShaderAsset requires explicit recipe and parameter types",
        );
      return;
    }
    if (isShader(type)) {
      shaderProps(props);
      return;
    }
    if (type === ENTITY_HOST_TYPE) {
      const owns = props.id !== undefined;
      const binds = props.bindTo !== undefined;
      const target = owns ? props.id : props.bindTo;
      if (owns === binds || typeof target !== "string" || target.length === 0) {
        throw new Error("Entity requires exactly one nonempty id or bindTo");
      }
      for (const key of Object.keys(props)) {
        if (!["id", "bindTo", "children", "ref"].includes(key)) {
          throw new Error(`Unsupported Entity prop: ${key}`);
        }
      }
      return;
    }
    if (type === CHILDREN_HOST_TYPE) {
      for (const key of Object.keys(props)) {
        if (!["children", "ref"].includes(key))
          throw new Error(`Unsupported Children prop: ${key}`);
      }
      return;
    }
    const descriptor = this.descriptor(type);
    const name = componentNames[type];
    if (props.bound != null && typeof props.bound !== "boolean") {
      throw new Error(`${name}.bound must be true, false, null, or undefined`);
    }
    for (const key of Object.keys(props)) {
      if (["bound", "ref", "children"].includes(key)) continue;
      if (
        type === componentContract.Surface.host &&
        key === "items" &&
        Array.isArray(props.items)
      ) {
        if (props.bound !== false || !this.client.encodeSurfaceItems)
          throw new Error(
            "Keyed Surface items require bound={false} and a Surface-capable client",
          );
        continue;
      }
      const field = Object.hasOwn(descriptor.fields, key)
        ? descriptor.fields[key]
        : undefined;
      if (!field) {
        if (!descriptor.dynamicProperties || key === "children")
          throw new Error(`Unsupported ${name} prop: ${key}`);
        if (!/^[A-Za-z_][A-Za-z_0-9]*$/.test(key))
          throw new Error(`Invalid dynamic property name: ${key}`);
        if (props[key] !== undefined) inferDynamicValue(props[key]);
        continue;
      }
      const value = props[key];
      const valid =
        value === undefined ||
        (key === "source" && isAssetReference(value)) ||
        (field.kind === FieldKind.Bytes
          ? value instanceof Uint8Array
          : typeof value ===
            (field.kind === FieldKind.String
              ? "string"
              : field.kind === FieldKind.Bool
                ? "boolean"
                : field.kind === FieldKind.U64 ||
                    field.kind === FieldKind.Entity
                  ? "bigint"
                  : "number"));
      if (!valid) {
        const expected =
          field.kind === FieldKind.Bytes
            ? "Uint8Array"
            : field.kind === FieldKind.String
              ? "string"
              : field.kind === FieldKind.Bool
                ? "boolean"
                : field.kind === FieldKind.U64 ||
                    field.kind === FieldKind.Entity
                  ? "bigint"
                  : "number";
        throw new Error(`${name}.${key} must be a ${expected} or undefined`);
      }
    }
  }

  private validateGui(type: GuiHostType, props: ReactWorldElementProps): void {
    const gui = props as unknown as Record<string, unknown>;
    validateGuiNodeRef((gui.nodeRef as unknown) ?? null);
    for (const key of ["onAction", "onActionCapture"] as const) {
      const listener = gui[key];
      if (listener !== undefined && typeof listener !== "function")
        throw new Error(`GUI ${key} must be a function`);
    }
    // Inline asset binding: a concrete source shared by two views creates
    // no duplicate asset, and swaps commit as ordinary style updates
    // without overlay teardown. Registry-id references stay a follow-up.
    const asset = gui.asset as unknown;
    if (asset !== undefined && asset !== null) {
      if (
        typeof asset !== "object" ||
        !Number.isInteger((asset as { kind: unknown }).kind) ||
        (asset as { kind: number }).kind <= 0 ||
        (asset as { kind: number }).kind > 65535 ||
        typeof (asset as { source: unknown }).source !== "string" ||
        (asset as { source: string }).source.length === 0 ||
        ((asset as { variant: unknown }).variant !== undefined &&
          (!Number.isInteger((asset as { variant: unknown }).variant) ||
            (asset as { variant: number }).variant < 0 ||
            (asset as { variant: number }).variant > 0xffffffff))
      )
        throw new Error(
          "GUI asset must be an asset source with kind, source and variant",
        );
    }
    if (gui.enabled !== undefined && typeof gui.enabled !== "boolean")
      throw new Error("GUI enabled must be a boolean or undefined");
    if (gui.theme !== undefined)
      validateGuiTheme(gui.theme as import("./gui/theme.js").GuiControlTheme);
    if (type === GUI_ROOT_HOST_TYPE) {
      if (gui.bound != null && typeof gui.bound !== "boolean")
        throw new Error(
          "GuiRoot.bound must be true, false, null, or undefined",
        );
      for (const key of Object.keys(props)) {
        if (
          ![
            "bound",
            "children",
            "nodeRef",
            "onAction",
            "onActionCapture",
          ].includes(key)
        )
          throw new Error(`Unsupported GuiRoot prop: ${key}`);
      }
      return;
    }
    if (gui.bound !== undefined)
      throw new Error("Only GuiRoot accepts a bound prop");
    const name = type;
    const finite = (key: string): void => {
      const value = gui[key];
      if (
        value !== undefined &&
        (typeof value !== "number" || !Number.isFinite(value))
      )
        throw new Error(`GUI ${key} must be a finite number or undefined`);
    };
    for (const key of [
      "width",
      "height",
      "minWidth",
      "minHeight",
      "maxWidth",
      "maxHeight",
      "flex",
      "alignX",
      "alignY",
    ])
      finite(key);
    const tuple = (
      key: string,
      length: 4 | 2,
      check: (value: number) => boolean,
      what: string,
    ): void => {
      const value = gui[key];
      if (value === undefined) return;
      if (
        !Array.isArray(value) ||
        value.length !== length ||
        !value.every((entry) => typeof entry === "number" && check(entry))
      )
        throw new Error(`GUI ${key} ${what}`);
    };
    const inUnit = (value: number): boolean =>
      Number.isFinite(value) && value >= 0 && value <= 1;
    tuple(
      "padding",
      4,
      (value) => Number.isFinite(value) && value >= 0,
      "must be four finite numbers >= 0",
    );
    tuple("margin", 4, Number.isFinite, "must be four finite numbers");
    tuple("color", 4, inUnit, "must be four finite numbers in 0..1");
    tuple("backgroundColor", 4, inUnit, "must be four finite numbers in 0..1");
    const opacity = gui.opacity;
    if (
      opacity !== undefined &&
      (typeof opacity !== "number" || !inUnit(opacity))
    )
      throw new Error("GUI opacity must be a finite number in 0..1");
    const fontSize = gui.fontSize;
    if (
      fontSize !== undefined &&
      (typeof fontSize !== "number" ||
        !Number.isFinite(fontSize) ||
        fontSize <= 0)
    )
      throw new Error("GUI fontSize must be a finite number > 0");
    const allowed = new Set([
      "width",
      "height",
      "minWidth",
      "minHeight",
      "maxWidth",
      "maxHeight",
      "padding",
      "margin",
      "flex",
      "alignX",
      "alignY",
      "color",
      "backgroundColor",
      "opacity",
      "fontSize",
      "asset",
      "enabled",
      "children",
      "nodeRef",
      "onAction",
      "onActionCapture",
      "theme",
    ]);
    if (type === GUI_TEXT_HOST_TYPE) {
      if (gui.text !== undefined && typeof gui.text !== "string")
        throw new Error("GUI Text text must be a string or undefined");
      allowed.add("text");
      if (gui.children != null)
        throw new Error(
          "GUI Text cannot contain declarations; pass text instead",
        );
    } else if (type === GUI_IMAGE_HOST_TYPE) {
      tuple(
        "size",
        2,
        (value) => Number.isFinite(value) && value > 0,
        "must be two finite numbers > 0",
      );
      allowed.add("size");
      if (gui.children != null)
        throw new Error("GUI Image cannot contain declarations");
    } else if (isGuiLeafHostType(type)) {
      if (gui.children != null)
        throw new Error(`GUI ${name} cannot contain declarations`);
    }
    switch (type) {
      case GUI_BUTTON_HOST_TYPE:
        validateButtonProps(gui as unknown as ButtonProps);
        allowed.add("label");
        allowed.add("onPress");
        break;
      case GUI_CHECKBOX_HOST_TYPE:
        validateCheckboxProps(gui as unknown as CheckboxProps);
        allowed.add("checked");
        allowed.add("onToggle");
        break;
      case GUI_SLIDER_HOST_TYPE:
        validateSliderProps(gui as unknown as SliderProps);
        allowed.add("value");
        allowed.add("min");
        allowed.add("max");
        allowed.add("step");
        allowed.add("onScalarCommit");
        break;
      case GUI_TEXT_INPUT_HOST_TYPE:
        validateTextInputProps(gui as unknown as TextInputProps);
        allowed.add("text");
        allowed.add("placeholder");
        allowed.add("onTextCommit");
        break;
      default:
        break;
    }
    for (const key of Object.keys(props)) {
      if (!allowed.has(key))
        throw new Error(`Unsupported ${name} prop: ${key}`);
    }
  }

  private descriptor(type: ReactWorldElementType): ComponentDescriptor {
    if (
      type === ANIMATION_HOST_TYPE ||
      isAsset(type) ||
      isShader(type) ||
      type === ENTITY_HOST_TYPE ||
      type === CHILDREN_HOST_TYPE ||
      isGuiHostType(type) ||
      !Object.hasOwn(componentNames, type)
    )
      throw new Error(`Unsupported element: ${type}`);
    const name = componentNames[type];
    const descriptor = this.components[name];
    if (!descriptor) throw new Error(`This runtime does not support ${name}`);
    if (
      Object.values(descriptor.fields).some(
        (field) => !declarationFieldKinds.has(field.kind),
      )
    )
      throw new Error(`Unsupported ${name} field kind`);
    return descriptor;
  }

  instance(
    type: ReactWorldElementType,
    props: ReactWorldElementProps,
  ): ReactWorldInstance {
    this.validate(type, props);
    return {
      identity: this.nextIdentity++,
      type,
      props,
      children: [],
      hidden: false,
    };
  }

  describe(): ReactWorldDescription {
    const assets: AssetDescription[] = [];
    const animationNodes: {
      instance: ReactWorldInstance;
      parent: number | undefined;
    }[] = [];
    const animationClips = new Map<
      string,
      import("@ipp/client").AnimationClipSource
    >();
    const entities: EntityOverlayBindingDescription[] = [];
    const overlays: ComponentStateOverlayDescription[] = [];
    const guiRoots = new Map<
      number,
      {
        entity: number;
        nodeRef: GuiNodeRef | null;
        nodes: GuiDescribedNodeWithCallbacks[];
      }
    >();
    const visit = (
      instance: ReactWorldInstance,
      parent?: number,
      hierarchyParent?: number,
      guiRoot?: number,
      guiParent?: number,
    ): void => {
      if (instance.hidden) return;
      let props = instance.props;
      this.validate(instance.type, props);
      if (instance.type === GUI_ROOT_HOST_TYPE) {
        if (parent === undefined || hierarchyParent !== undefined)
          throw new Error("GuiRoot must be directly inside an Entity");
        if (guiRoot !== undefined)
          throw new Error("GuiRoot cannot be nested inside another GuiRoot");
        if (!this.components["GuiRoot"])
          throw new Error("This runtime does not support GuiRoot");
        // Root ownership is producer-side. The commit phase creates the
        // GuiRoot component with insertComponent, drives its node tree only
        // through incremental GUI edits, and removes the producer with
        // removeComponent on unmount. No overlay is described here: core
        // rejects overlay declarations that would create a GuiRoot or write
        // its node tree, so Auto/Owned root overlays always fail, and any
        // property-lane overlay must stay Bound. The accepted `bound` prop
        // keeps declaration compatibility and selects no overlay mode.
        const record: {
          entity: number;
          nodeRef: GuiNodeRef | null;
          nodes: GuiDescribedNodeWithCallbacks[];
        } = {
          entity: parent,
          nodeRef: (props as unknown as GuiNodeProps).nodeRef ?? null,
          nodes: [],
        };
        guiRoots.set(instance.identity, record);
        let roots = 0;
        for (const child of instance.children) {
          if (child.hidden) continue;
          roots += 1;
          visit(child, parent, hierarchyParent, instance.identity, undefined);
        }
        if (roots > 1) throw new Error("GuiRoot owns exactly one root node");
        return;
      }
      if (isGuiHostType(instance.type)) {
        if (guiRoot === undefined || parent === undefined)
          throw new Error("GUI nodes must be inside a GuiRoot");
        const record = guiRoots.get(guiRoot);
        if (!record) throw new Error("Missing GUI root declaration");
        const nodeProps = props as unknown as GuiNodeProps & {
          text?: string | undefined;
          size?: readonly [number, number] | undefined;
          label?: string | undefined;
          checked?: boolean | undefined;
          value?: number | undefined;
          min?: number | undefined;
          max?: number | undefined;
          step?: number | undefined;
          placeholder?: string | undefined;
        };
        // Control listeners are validated with their control props and held
        // JS-only on the described node (see GuiRetainedNodeCallbacks): they
        // never reach the commit signature, the diff, or any transport.
        const controlListeners = props as unknown as {
          onPress?: GuiPressListener | undefined;
          onToggle?: GuiToggleListener | undefined;
          onScalarCommit?: GuiScalarCommitListener | undefined;
          onTextCommit?: GuiTextCommitListener | undefined;
        };
        const callbacks: GuiRetainedNodeCallbacks = {
          onAction: nodeProps.onAction as GuiActionListener | undefined,
          onActionCapture: nodeProps.onActionCapture as
            | GuiActionListener
            | undefined,
          onPress: controlListeners.onPress,
          onToggle: controlListeners.onToggle,
          onScalarCommit: controlListeners.onScalarCommit,
          onTextCommit: controlListeners.onTextCommit,
        };
        const described: GuiDescribedNodeWithCallbacks = {
          identity: instance.identity,
          parent: guiParent,
          type: instance.type,
          content: guiContentFor(instance.type, nodeProps),
          style: guiStyleWithTheme(guiStyleFor(nodeProps), nodeProps.theme),
          nodeRef: (nodeProps.nodeRef as GuiNodeRef | null | undefined) ?? null,
          onAction: callbacks.onAction,
          onActionCapture: callbacks.onActionCapture,
          ...(nodeProps.theme === undefined ? {} : { theme: nodeProps.theme }),
          callbackSeam: callbacks,
        };
        record.nodes.push(described);
        if (isGuiLeafHostType(instance.type)) {
          if (instance.children.some((child) => !child.hidden))
            throw new Error("GUI leaves cannot contain declarations");
          return;
        }
        for (const child of instance.children) {
          if (child.hidden) continue;
          visit(child, parent, hierarchyParent, guiRoot, instance.identity);
        }
        return;
      }
      if (guiRoot !== undefined)
        throw new Error("Only GUI declarations can be inside a GuiRoot");
      if (
        instance.type === componentContract.Surface.host &&
        Array.isArray(props.items)
      ) {
        let declarations = this.surfaceItems.get(instance);
        if (!declarations) {
          declarations = new SurfaceItemDeclarations();
          this.surfaceItems.set(instance, declarations);
        }
        const described = declarations.describe(
          props.items as SurfaceItemProps[],
          (collection) => this.client.encodeSurfaceItems!(collection),
        );
        this.surfaceBytes.add(described.items);
        props = {
          ...props,
          ...described,
        };
      }
      if (instance.type === ANIMATION_HOST_TYPE) {
        if (instance.children.length)
          throw new Error("Animation cannot contain declarations");
        animationNodes.push({ instance, parent });
        return;
      }
      if (isAsset(instance.type)) {
        let encoded: ReturnType<ReactWorldTree["encodedAsset"]>;
        let kind: number;
        if (instance.type === SHADER_ASSET_HOST_TYPE) {
          const shader = props as unknown as ShaderAssetProps;
          const stages = instance.children
            .filter((child) => !child.hidden)
            .map((child) => {
              if (!isShader(child.type))
                throw new Error("ShaderAsset children must be shader stages");
              return { type: child.type, props: shaderProps(child.props) };
            });
          encoded = this.encodedAsset(
            instance,
            [
              encodeShaderDefinition,
              shader.parameters,
              JSON.stringify(shader.recipe),
              ...stages.flatMap(({ type, props }) => [
                type,
                props.children,
                props.backend,
                props.requiredAttributes,
                props.references,
              ]),
            ],
            () =>
              encodeShaderDefinition(
                describeShader(stages, shader.parameters, shader.recipe),
              ),
          );
          kind = 13;
        } else if (instance.type === ANIMATION_ASSET_HOST_TYPE) {
          if (!this.client.encodeAnimationClip)
            throw new Error(
              "AnimationAsset requires an animation-capable client",
            );
          encoded = this.encodedAsset(
            instance,
            [this.client.encodeAnimationClip, props.clip],
            () =>
              this.client.encodeAnimationClip!(
                props.clip as import("@ipp/client").AnimationClipSource,
              ),
          );
          kind = 10;
          animationClips.set(
            props.id as string,
            props.clip as import("@ipp/client").AnimationClipSource,
          );
        } else {
          const asset = props as unknown as AssetProps<unknown>;
          encoded = this.encodedAsset(
            instance,
            [asset.encode, asset.data],
            () => asset.encode(asset.data),
          );
          kind = asset.kind;
        }
        const variant = (props.variant ?? 0) as number;
        if (!Number.isInteger(variant) || variant < 0 || variant > 0xffffffff)
          throw new Error("Invalid asset variant");
        assets.push({
          identity: instance.identity,
          id: props.id as string,
          kind,
          variant,
          bytes: encoded.bytes,
          signature: encoded.signature,
          version: encoded.version,
        });
        if (instance.type === ASSET_HOST_TYPE)
          for (const child of instance.children)
            visit(child, parent, hierarchyParent);
        return;
      }
      if (hierarchyParent !== undefined && instance.type !== ENTITY_HOST_TYPE)
        throw new Error("Children must contain Entity declarations");
      if (instance.type === CHILDREN_HOST_TYPE) {
        if (parent === undefined)
          throw new Error("Children must be inside an Entity");
        for (const child of instance.children) visit(child, parent, parent);
        return;
      }
      if (instance.type === ENTITY_HOST_TYPE) {
        entities.push({
          identity: instance.identity,
          symbolicId: (props.id ?? props.bindTo) as string,
          mode: props.id === undefined ? "bound" : "owned",
        });
        if (hierarchyParent !== undefined) {
          const descriptor = this.descriptor(componentContract.Hierarchy.host);
          overlays.push({
            // Host identities are positive; each Entity has at most one implicit parent overlay.
            identity: -instance.identity,
            entity: instance.identity,
            component: descriptor.id,
            mode: "auto",
            fields: new Map([
              [
                descriptor.fields.parent!.offset,
                { kind: "binding", value: hierarchyParent },
              ],
            ]),
          });
        }
        for (const child of instance.children) visit(child, instance.identity);
        return;
      }
      if (isShader(instance.type))
        throw new Error("Shader declarations must be children of ShaderAsset");
      if (parent === undefined)
        throw new Error(
          `${componentNames[instance.type]} must be inside an Entity`,
        );
      const descriptor = this.descriptor(instance.type);
      const fields = new Map<number, ReactWorldFieldValue>();
      for (const [name, field] of Object.entries(descriptor.fields)) {
        const value = props[name];
        if (value === undefined) continue;
        if (name === "source" && isAssetReference(value)) {
          fields.set(field.offset, { kind: "asset", value: value.assetId });
          continue;
        }
        fields.set(
          field.offset,
          field.kind === FieldKind.String
            ? { kind: "string", value: value as string }
            : field.kind === FieldKind.Bool
              ? { kind: "bool", value: value as boolean }
              : field.kind === FieldKind.Bytes
                ? {
                    kind: "bytes",
                    value: value as Uint8Array<ArrayBuffer>,
                  }
                : field.kind === FieldKind.U64 ||
                    field.kind === FieldKind.Entity
                  ? field.kind === FieldKind.Entity
                    ? {
                        kind: "entity",
                        value: { kind: "handle", id: value as bigint },
                      }
                    : { kind: "u64", value: value as bigint }
                  : {
                      kind: field.kind === FieldKind.U32 ? "u32" : "f32",
                      value: value as number,
                    },
        );
      }
      const properties = descriptor.dynamicProperties
        ? Object.fromEntries(
            Object.entries(props)
              .filter(
                ([name, value]) =>
                  value !== undefined &&
                  !["bound", "ref", "children"].includes(name) &&
                  !Object.hasOwn(descriptor.fields, name),
              )
              .map(([name, value]) => [name, inferDynamicValue(value)]),
          )
        : undefined;
      for (const child of instance.children) {
        if (!child.hidden && !isAsset(child.type))
          throw new Error(
            "Component children must be Asset declarations; shader stages require ShaderAsset",
          );
        visit(child, parent);
      }
      overlays.push({
        identity: instance.identity,
        entity: parent,
        component: descriptor.id,
        mode: props.bound == null ? "auto" : props.bound ? "bound" : "owned",
        fields,
        ...(properties ? { properties } : {}),
      });
    };
    for (const child of this.children) visit(child);

    const ids = new Set<string>();
    for (const asset of assets) {
      if (ids.has(asset.id)) throw new Error(`Duplicate asset id: ${asset.id}`);
      ids.add(asset.id);
    }
    for (const overlay of overlays)
      for (const field of overlay.fields.values()) {
        if (field.kind === "asset" && !ids.has(field.value))
          throw new Error(`Unknown asset id: ${field.value}`);
      }
    const animationAssets = new Map(
      assets.map((asset) => [
        asset.id,
        {
          kind: asset.kind,
          ...(animationClips.has(asset.id)
            ? { clip: animationClips.get(asset.id)! }
            : {}),
        },
      ]),
    );
    const animations = animationNodes.map(({ instance, parent }) =>
      describeAnimation(
        instance.identity,
        instance.props as unknown as AnimationProps & {
          mailbox: AnimationMailbox;
        },
        parent,
        entities,
        animationAssets,
      ),
    );
    const parented = new Set(
      overlays
        .filter((overlay) => overlay.identity < 0)
        .map((overlay) => overlay.entity),
    );
    for (const overlay of overlays) {
      if (
        overlay.identity > 0 &&
        parented.has(overlay.entity) &&
        overlay.component === this.components.Hierarchy?.id
      )
        throw new Error(
          "An Entity inside Children already has a Hierarchy declaration",
        );
    }

    const gui: GuiDescribedRoot[] = [...guiRoots].map(([identity, record]) => ({
      identity,
      entity: record.entity,
      nodeRef: record.nodeRef,
      nodes: record.nodes,
      signature: guiRootSignature(record.nodes),
    }));
    // Preserve NaN, infinities and -0 so encoder-rejected authored values
    // cannot compare equal to a later corrected declaration.
    const signature = JSON.stringify([
      assets.map(({ bytes, signature, ...asset }) => asset),
      animations.map(({ mailbox, onPlaybackEvent, ...description }) =>
        animationSignature(description),
      ),
      entities,
      overlays.map((overlay) => ({
        ...overlay,
        fields: [...overlay.fields].map(([offset, value]) => [
          offset,
          value.kind,
          value.kind === "entity"
            ? value.value.kind === "handle"
              ? String(value.value.id)
              : `alias:${value.value.alias}`
            : value.kind === "bytes"
              ? this.byteSignature(value.value)
              : Object.is(value.value, -0)
                ? "-0"
                : String(value.value),
        ]),
      })),
      gui.map((root) => [root.identity, root.entity, root.signature]),
    ]);
    return { assets, animations, entities, overlays, gui, signature };
  }
}

export function remove(
  parent: { children: ReactWorldInstance[] },
  child: ReactWorldInstance,
): void {
  const index = parent.children.indexOf(child);
  if (index !== -1) parent.children.splice(index, 1);
}

export function insert(
  parent: { children: ReactWorldInstance[] },
  child: ReactWorldInstance,
  before?: ReactWorldInstance,
): void {
  remove(parent, child);
  const index =
    before === undefined
      ? parent.children.length
      : parent.children.indexOf(before);
  if (index < 0) throw new Error("Invalid React insertion point");
  parent.children.splice(index, 0, child);
}
