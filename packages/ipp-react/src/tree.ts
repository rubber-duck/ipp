import {
  declarationFields,
  fieldIdentity,
  type DeclarationFieldValue,
} from "./field_values.js";
import { guiControlNames } from "./gui/manifest.js";
import {
  controlCallbackNames,
  controlFeedbackCallbackNames,
  controlValueCallbackNames,
  type GuiActionListeners,
  type GuiControlListeners,
} from "./gui/callbacks.js";
import { validateControlRef, type GuiControlRef } from "./gui/control-ref.js";
import {
  ANIMATION_HOST_TYPE,
  type AnimationDescription,
  type AnimationProps,
  type AnimationMailbox,
} from "./animation.js";
import { describeAnimation, animationSignature } from "./animation_tree.js";
import type { ReactEntityReference } from "./entity_references.js";
import { attachmentIdentity } from "./attachment-identity.js";
import {
  ATTACHED_WORLD_HOST_TYPE,
  describeAttachedWorld,
  type AttachedWorldDescription,
} from "./attached-world.js";
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
import type { ReactWorldClient } from "./contract.js";
import {
  FieldKind,
  inferDynamicValue,
  type DynamicValue,
  encodeShaderDefinition,
  type ComponentDescriptor,
  type FieldKind as FieldKindValue,
} from "@ipp/client";
import {
  componentNames,
  ENTITY_HOST_TYPE,
  CHILDREN_HOST_TYPE,
  ENTITY_LINK_HOST_TYPE,
  type ReactWorldComponentType,
} from "./components.js";
export type ReactWorldElementType =
  | typeof ATTACHED_WORLD_HOST_TYPE
  | typeof ANIMATION_HOST_TYPE
  | typeof CHILDREN_HOST_TYPE
  | typeof ENTITY_LINK_HOST_TYPE
  | typeof ENTITY_HOST_TYPE
  | ReactWorldComponentType
  | ShaderHostType
  | AssetHostType;
export type ReactWorldFieldValue =
  | DeclarationFieldValue
  | { kind: "asset"; value: string }
  | { kind: "row-asset"; value: string }
  | { kind: "entity-reference"; value: string | ReactEntityReference };
export type ReactWorldElementProps = Readonly<Record<string, unknown>>;

const declarationFieldKinds: ReadonlySet<FieldKindValue> = new Set([
  FieldKind.F32,
  FieldKind.U32,
  FieldKind.U64,
  FieldKind.Entity,
  FieldKind.String,
  FieldKind.Bytes,
  FieldKind.Rows,
  FieldKind.Bool,
]);

export interface ReactWorldInstance {
  readonly identity: number;
  readonly type: ReactWorldElementType;
  /** The tree whose container committed this instance. */
  readonly tree: ReactWorldTree;
  props: ReactWorldElementProps;
  children: ReactWorldInstance[];
  hidden: boolean;
}

/**
 * A render declares two `<Entity id>` nodes with the same symbolic id. Like
 * other invalid trees, the render is rejected locally before anything is
 * sent. An id may still move to another node across renders.
 */
export class ReactWorldDuplicateEntityError extends Error {
  constructor(readonly symbolicId: string) {
    super(`Duplicate Entity id: ${symbolicId}`);
    this.name = "ReactWorldDuplicateEntityError";
  }
}

/** One `<Entity>` declaration. */
export interface ReactEntityDescription extends GuiActionListeners {
  readonly identity: number;
  readonly symbolicId: string;
  /**
   * `declared` for `<Entity id>`: the root creates or adopts the entity with
   * this symbolic id and deletes it when the declaration disappears. `bound`
   * for `<Entity bindTo>`: the root only refers to an existing entity.
   */
  readonly kind: "declared" | "bound";
  /** The enclosing Entity declaration in React's element tree. */
  readonly parent: number | undefined;
}

/** One component declaration on the Entity declaration `entity`. */
export interface ReactComponentDescription {
  readonly identity: number;
  readonly entity: number;
  readonly component: number;
  /** Declared field values by offset; an omitted prop is not written. */
  readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  readonly properties?: Readonly<Record<string, DynamicValue>>;
  /** Set exactly on GUI control components. */
  readonly control?: true;
  readonly controlRef?: GuiControlRef | undefined;
  /**
   * The callbacks of a control or a Behavior, always present on one, whose
   * values the root observes for its callbacks and action listeners.
   */
  readonly controlListeners?: GuiControlListeners | undefined;
}

/** One `<Children>` or `<EntityLink>` placement of the Entity `entity`. */
export interface ReactEntityLinkDescription {
  readonly identity: number;
  readonly entity: number;
  readonly parent: ReactEntityReference | null;
  readonly before: ReactEntityReference | null;
}

export interface ReactWorldDescription {
  /** Some Entity declares `onAction` or `onActionCapture`. */
  readonly guiActions?: boolean;
  /** Momentary GUI effects (press, submit, actions) have listeners. */
  readonly guiEffects?: boolean;
  /** Feedback effects (focus and pointer interaction changes) have listeners. */
  readonly guiFeedback?: boolean;
  readonly attachments?: readonly AttachedWorldDescription[];
  readonly animations: readonly AnimationDescription[];
  readonly assets: readonly AssetDescription[];
  readonly entities: readonly ReactEntityDescription[];
  readonly components: readonly ReactComponentDescription[];
  readonly links: readonly ReactEntityLinkDescription[];
  readonly signature: string;
  /** This description's position among its tree's descriptions. */
  readonly serial?: number;
  /**
   * Present when this description differs from the tree's previous one, of
   * serial `base`, only in the listed declarations: no declaration appeared,
   * disappeared, moved, changed its symbolic id or gained or lost a
   * listener. A consumer that saw that description updates only these.
   */
  readonly changes?: ReactWorldDescriptionChanges;
}

/** The declarations a description changed, by their position in it. */
export interface ReactWorldDescriptionChanges {
  readonly base: number;
  readonly entities: ReadonlyMap<number, ReactEntityDescription>;
  readonly components: ReadonlyMap<number, ReactComponentDescription>;
}

/**
 * Running totals of commit work in this module, which tests compare across a
 * commit: `described` counts each instance whose declaration a describe
 * visited, `compared` each component declaration whose fields a commit
 * compared with the acknowledged ones.
 */
export const commitCounters = { described: 0, compared: 0 };

/** Where the last full describe placed an instance. */
type InstanceRole =
  | {
      readonly kind: "entity" | "component" | "asset" | "attachment";
      readonly index: number;
    }
  | {
      readonly kind: "animation";
      readonly index: number;
      readonly parent: number | undefined;
    }
  | { readonly kind: "stage"; readonly asset: ReactWorldInstance }
  | { readonly kind: "placement" };

/** The last description and what describing a commit's changes needs of it. */
interface DescribedTree {
  readonly description: ReactWorldDescription;
  readonly resources: string;
  /** Each instance the last full describe visited. */
  readonly roles: ReadonlyMap<ReactWorldInstance, InstanceRole>;
  readonly assetIds: ReadonlySet<string>;
  readonly animationAssets: ReadonlyMap<
    string,
    { kind: number; clip?: import("@ipp/client").AnimationClipSource }
  >;
  readonly reference: (value: string | bigint) => ReactEntityReference;
}

export class ReactWorldTree {
  children: ReactWorldInstance[] = [];
  /** A commit inserted, removed, hid or revealed an instance. */
  private structural = true;
  /** Instances whose props a commit replaced since the last describe. */
  private readonly touched = new Set<ReactWorldInstance>();
  private readonly components: ReactWorldClient["components"];
  private nextIdentity = 1;
  private nextAssetVersion = 1;
  private nextSerial = 1;
  private revision = 0;
  private described: DescribedTree | undefined;
  private readonly descriptors = new Map<
    ReactWorldElementType,
    ComponentDescriptor
  >();
  /** The props each instance last validated. */
  private readonly validated = new WeakMap<
    ReactWorldInstance,
    ReactWorldElementProps
  >();
  private readonly entityDescriptions = new WeakMap<
    ReactWorldInstance,
    {
      props: ReactWorldElementProps;
      parent: number | undefined;
      description: ReactEntityDescription;
    }
  >();
  private readonly componentDescriptions = new WeakMap<
    ReactWorldInstance,
    {
      props: ReactWorldElementProps;
      structure: ComponentShape;
      description: ReactComponentDescription;
    }
  >();
  private readonly attachmentDescriptions = new WeakMap<
    ReactWorldInstance,
    { props: ReactWorldElementProps; description: AttachedWorldDescription }
  >();
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

  /** Whether a commit changed this tree since its last description. */
  get changed(): boolean {
    return this.structural || this.touched.size > 0;
  }

  /** A commit replaced the props of `instance`. */
  touch(instance: ReactWorldInstance): void {
    this.touched.add(instance);
  }

  /** A commit inserted, removed, hid or revealed instances. */
  restructure(): void {
    this.structural = true;
  }

  validate(type: ReactWorldElementType, props: ReactWorldElementProps): void {
    if (type === ATTACHED_WORLD_HOST_TYPE) {
      describeAttachedWorld(props);
      return;
    }
    if (props.ref != null) throw new Error("Remote refs are not supported yet");
    if (type === ANIMATION_HOST_TYPE) {
      this.requireOperation("animation");
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
        if (
          ![
            "id",
            "bindTo",
            "children",
            "ref",
            "onAction",
            "onActionCapture",
          ].includes(key)
        ) {
          throw new Error(`Unsupported Entity prop: ${key}`);
        }
      }
      for (const key of ["onAction", "onActionCapture"] as const)
        if (props[key] !== undefined && typeof props[key] !== "function")
          throw new Error(`${key} must be a callback`);
      return;
    }
    if (type === CHILDREN_HOST_TYPE) {
      this.requireOperation("entityLinks");
      for (const key of Object.keys(props)) {
        if (!["children", "ref"].includes(key))
          throw new Error(`Unsupported Children prop: ${key}`);
      }
      return;
    }
    if (type === ENTITY_LINK_HOST_TYPE) {
      this.requireOperation("entityLinks");
      for (const key of Object.keys(props))
        if (!["parent", "before", "ref"].includes(key))
          throw new Error(`Unsupported EntityLink prop: ${key}`);
      for (const key of ["parent", "before"] as const) {
        const value = props[key];
        if (key === "before" && value === undefined) continue;
        if (
          value !== null &&
          typeof value !== "bigint" &&
          !(typeof value === "string" && value.length > 0)
        )
          throw new Error(
            `EntityLink.${key} must be an Entity reference or null`,
          );
      }
      return;
    }
    const descriptor = this.descriptor(type);
    const name = componentNames[type];
    // Every control receives context requests and feedback.
    const roleCallbacks: readonly string[] =
      name === "GuiButton"
        ? ["onPress", "onSelectedChange"]
        : name === "GuiCheckbox"
          ? ["onToggle"]
          : name === "GuiSlider"
            ? ["onScalarCommit"]
            : name === "GuiColor"
              ? ["onColorCommit"]
              : name === "GuiTextInput"
                ? [
                    "onTextCommit",
                    "onScalarCommit",
                    "onSubmit",
                    "onReject",
                    "onDiscard",
                  ]
                : name === "GuiScrollView" || name === "GuiVirtualList"
                  ? ["onRangeChange", "onScroll"]
                  : name === "GuiBehavior"
                    ? ["onVisibleChange"]
                    : [];
    const supportedCallbacks = guiControlNames.has(name)
      ? [...roleCallbacks, "onContextMenu", ...controlFeedbackCallbackNames]
      : roleCallbacks;
    for (const key of controlCallbackNames) {
      if (props[key] === undefined) continue;
      if (!supportedCallbacks.includes(key) || typeof props[key] !== "function")
        throw new Error(`Unsupported ${name} callback: ${key}`);
    }
    declarationFields(props.fields);
    if (props.controlRef !== undefined) {
      if (!guiControlNames.has(name))
        throw new Error("Only controls accept control refs");
      validateControlRef(props.controlRef);
      if (
        props.controlRef &&
        (!this.client.inspectPage || !this.client.watchLifecycle)
      )
        throw new Error("Control refs require the ordinary GUI client");
      if (props.controlRef) this.requireLifecyclePublisher("Control refs");
    }
    for (const key of Object.keys(props)) {
      if (
        [
          "ref",
          "children",
          "fields",
          "controlRef",
          ...supportedCallbacks,
        ].includes(key)
      )
        continue;
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
        (field.kind === FieldKind.Entity &&
          typeof value === "string" &&
          value.length > 0) ||
        (field.kind === FieldKind.Bytes || field.kind === FieldKind.Rows
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

  private requireLifecyclePublisher(feature: string): void {
    if (
      this.client.manifest &&
      !this.client.manifest.systems.includes("ipp.lifecycle-publisher")
    )
      throw new Error(`${feature} require the selected lifecycle publisher`);
  }

  private requireOperation(
    operation: NonNullable<ReactWorldClient["manifest"]>["operations"][number],
  ): void {
    if (
      this.client.manifest &&
      !this.client.manifest.operations.includes(operation)
    )
      throw new Error(`This World does not select ${operation}`);
  }

  private descriptor(type: ReactWorldElementType): ComponentDescriptor {
    const cached = this.descriptors.get(type);
    if (cached) return cached;
    if (
      type === ATTACHED_WORLD_HOST_TYPE ||
      type === ANIMATION_HOST_TYPE ||
      isAsset(type) ||
      isShader(type) ||
      type === ENTITY_HOST_TYPE ||
      type === CHILDREN_HOST_TYPE ||
      type === ENTITY_LINK_HOST_TYPE ||
      !Object.hasOwn(componentNames, type)
    )
      throw new Error(`Unsupported element: ${type}`);
    const name = componentNames[type];
    const descriptor = this.components[name];
    if (!descriptor) throw new Error(`This runtime does not support ${name}`);
    if (
      this.client.manifest &&
      !this.client.manifest.components.includes(descriptor.id)
    )
      throw new Error(`This World does not select ${name}`);
    if (
      Object.values(descriptor.fields).some(
        (field) => !declarationFieldKinds.has(field.kind),
      )
    )
      throw new Error(`Unsupported ${name} field kind`);
    this.descriptors.set(type, descriptor);
    return descriptor;
  }

  instance(
    type: ReactWorldElementType,
    props: ReactWorldElementProps,
  ): ReactWorldInstance {
    if (type !== ATTACHED_WORLD_HOST_TYPE) this.validate(type, props);
    const instance: ReactWorldInstance = {
      identity: this.nextIdentity++,
      type,
      tree: this,
      props,
      children: [],
      hidden: false,
    };
    this.validated.set(instance, props);
    return instance;
  }

  /** Validate props that changed a declared value since their last check. */
  private validateOnce(instance: ReactWorldInstance): void {
    const previous = this.validated.get(instance);
    if (previous === instance.props) return;
    if (!previous || !sameDeclarationProps(previous, instance.props))
      this.validate(instance.type, instance.props);
    this.validated.set(instance, instance.props);
  }

  private describeEntity(
    instance: ReactWorldInstance,
    parent: number | undefined,
  ): ReactEntityDescription {
    const props = instance.props;
    const cached = this.entityDescriptions.get(instance);
    if (cached?.props === props && cached.parent === parent)
      return cached.description;
    this.validateOnce(instance);
    const symbolicId = (props.id ?? props.bindTo) as string;
    const kind: ReactEntityDescription["kind"] =
      props.id === undefined ? "bound" : "declared";
    const onAction = props.onAction as GuiActionListeners["onAction"];
    const onActionCapture =
      props.onActionCapture as GuiActionListeners["onActionCapture"];
    const previous = cached?.description;
    const description =
      previous &&
      previous.symbolicId === symbolicId &&
      previous.kind === kind &&
      previous.parent === parent &&
      previous.onAction === onAction &&
      previous.onActionCapture === onActionCapture
        ? previous
        : {
            identity: instance.identity,
            symbolicId,
            kind,
            parent,
            onAction,
            onActionCapture,
          };
    this.entityDescriptions.set(instance, { props, parent, description });
    return description;
  }

  /**
   * Describe one component declaration from its own props. Unchanged values
   * keep their field map and property objects, so the commit diff and change
   * detection compare them by identity instead of by value.
   */
  private describeComponent(
    instance: ReactWorldInstance,
    parent: number,
  ): { description: ReactComponentDescription; structure: ComponentShape } {
    const props = instance.props;
    const cached = this.componentDescriptions.get(instance);
    if (cached?.props === props && cached.description.entity === parent)
      return cached;
    let structure = cached?.structure;
    this.validateOnce(instance);
    if (!cached || !sameDeclarationProps(cached.props, props)) {
      const next = this.componentShape(instance.type, props);
      if (!structure || !sameShape(structure, next)) structure = next;
    }
    const name = componentNames[instance.type as ReactWorldComponentType];
    const control = guiControlNames.has(name);
    const controlRef = props.controlRef as GuiControlRef | undefined;
    const controlListeners =
      control || name === "GuiBehavior"
        ? (Object.fromEntries(
            controlCallbackNames
              .filter((key) => props[key] !== undefined)
              .map((key) => [key, props[key]]),
          ) as GuiControlListeners)
        : undefined;
    const previous = cached?.description;
    const description: ReactComponentDescription =
      previous &&
      previous.entity === parent &&
      previous.fields === structure!.fields &&
      previous.properties === structure!.properties &&
      previous.controlRef === controlRef &&
      sameListeners(previous.controlListeners, controlListeners)
        ? previous
        : {
            identity: instance.identity,
            entity: parent,
            component: structure!.component,
            fields: structure!.fields,
            ...(control
              ? { control: true as const, controlRef, controlListeners }
              : controlListeners
                ? { controlListeners }
                : {}),
            ...(structure!.properties
              ? { properties: structure!.properties }
              : {}),
          };
    const result = { props, structure: structure!, description };
    this.componentDescriptions.set(instance, result);
    return result;
  }

  private componentShape(
    type: ReactWorldElementType,
    props: ReactWorldElementProps,
  ): ComponentShape {
    const descriptor = this.descriptor(type);
    const fields = new Map<number, ReactWorldFieldValue>();
    const assetIds: string[] = [];
    let entityReferences = false;
    for (const [name, field] of Object.entries(descriptor.fields)) {
      const value = props[name];
      if (value === undefined) continue;
      if (name === "source" && isAssetReference(value)) {
        fields.set(field.offset, { kind: "asset", value: value.assetId });
        assetIds.push(value.assetId);
        continue;
      }
      if (field.kind === FieldKind.Entity && typeof value === "string") {
        fields.set(field.offset, { kind: "entity-reference", value });
        entityReferences = true;
        continue;
      }
      fields.set(
        field.offset,
        field.kind === FieldKind.String
          ? { kind: "string", value: value as string }
          : field.kind === FieldKind.Bool
            ? { kind: "bool", value: value as boolean }
            : field.kind === FieldKind.Bytes || field.kind === FieldKind.Rows
              ? {
                  kind: field.kind === FieldKind.Rows ? "rows" : "bytes",
                  value: (value as Uint8Array<ArrayBuffer>).slice(),
                }
              : field.kind === FieldKind.U64 || field.kind === FieldKind.Entity
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
    for (const write of declarationFields(props.fields)) {
      if (fields.has(write.offset))
        throw new Error("Component prop and FieldWrite overlap");
      if ("asset" in write) assetIds.push(write.asset.assetId);
      fields.set(
        write.offset,
        "asset" in write
          ? { kind: "row-asset", value: write.asset.assetId }
          : (structuredClone(write.value) as DeclarationFieldValue),
      );
    }
    const properties = descriptor.dynamicProperties
      ? Object.fromEntries(
          Object.entries(props)
            .filter(
              ([name, value]) =>
                value !== undefined &&
                ![
                  "ref",
                  "children",
                  "fields",
                  "controlRef",
                  ...controlCallbackNames,
                ].includes(name) &&
                !Object.hasOwn(descriptor.fields, name),
            )
            .map(([name, value]) => [name, inferDynamicValue(value)]),
        )
      : undefined;
    return {
      component: descriptor.id,
      fields,
      properties,
      assetIds,
      entityReferences,
    };
  }

  private describeAttachment(
    instance: ReactWorldInstance,
    suspended: boolean,
  ): AttachedWorldDescription {
    let cached = this.attachmentDescriptions.get(instance);
    if (cached?.props !== instance.props) {
      cached = {
        props: instance.props,
        description: describeAttachedWorld(instance.props),
      };
      this.attachmentDescriptions.set(instance, cached);
    }
    return { ...cached.description, suspended };
  }

  /** Describe one asset declaration, encoding it only when its inputs changed. */
  private describeAsset(instance: ReactWorldInstance): {
    description: AssetDescription;
    clip: import("@ipp/client").AnimationClipSource | undefined;
  } {
    this.validateOnce(instance);
    const props = instance.props;
    let encoded: ReturnType<ReactWorldTree["encodedAsset"]>;
    let kind: number;
    let clip: import("@ipp/client").AnimationClipSource | undefined;
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
          // Parameter and recipe tables are plain data that renders
          // commonly pass as fresh literals; compare them by value.
          JSON.stringify(shader.parameters),
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
        throw new Error("AnimationAsset requires an animation-capable client");
      encoded = this.encodedAsset(
        instance,
        [this.client.encodeAnimationClip, props.clip],
        () =>
          this.client.encodeAnimationClip!(
            props.clip as import("@ipp/client").AnimationClipSource,
          ),
      );
      kind = 10;
      clip = props.clip as import("@ipp/client").AnimationClipSource;
    } else {
      const asset = props as unknown as AssetProps<unknown>;
      encoded = this.encodedAsset(instance, [asset.encode, asset.data], () =>
        asset.encode(asset.data),
      );
      kind = asset.kind;
    }
    const variant = (props.variant ?? 0) as number;
    if (!Number.isInteger(variant) || variant < 0 || variant > 0xffffffff)
      throw new Error("Invalid asset variant");
    return {
      description: {
        identity: instance.identity,
        id: props.id as string,
        kind,
        variant,
        bytes: encoded.bytes,
        signature: encoded.signature,
        version: encoded.version,
      },
      clip,
    };
  }

  /** Reject animation bindings this World cannot evaluate. */
  private checkAnimation(animation: AnimationDescription): void {
    for (const binding of animation.bindings) {
      if (binding.property.entityLink) this.requireOperation("entityLinks");
      if (binding.property.joints) this.requireOperation("jointAnimation");
      const component = binding.property.component;
      if (
        component !== undefined &&
        this.client.manifest &&
        !this.client.manifest.components.includes(component)
      )
        throw new Error(
          `This World does not select animation component ${component}`,
        );
    }
  }

  /**
   * Describe a commit. When it only replaced props, only the instances it
   * touched are described again, and the result names the declarations that
   * changed; a commit that inserted, removed, hid or revealed instances, or
   * changed what other declarations depend on, describes the whole tree.
   * Returns the previous description itself when nothing described changed.
   */
  describeCommit(): ReactWorldDescription {
    if (this.structural || !this.described) return this.describe();
    let description: ReactWorldDescription | undefined;
    try {
      description = this.describeTouched(this.described);
    } catch (error) {
      this.touched.clear();
      this.structural = true;
      throw error;
    }
    if (!description) return this.describe();
    this.touched.clear();
    return description;
  }

  private describeTouched(
    described: DescribedTree,
  ): ReactWorldDescription | undefined {
    const previous = described.description;
    const entities = new Copied(previous.entities);
    const components = new Copied(previous.components);
    const assets = new Copied(previous.assets);
    const animations = new Copied(previous.animations);
    const attachments = new Copied(previous.attachments ?? []);
    const changedEntities = new Map<number, ReactEntityDescription>();
    const changedComponents = new Map<number, ReactComponentDescription>();
    const touchedAssets = new Set<ReactWorldInstance>();
    let revised = false;
    for (const instance of this.touched) {
      const role = described.roles.get(instance);
      // A hidden or removed instance is not described.
      if (!role) continue;
      commitCounters.described++;
      if (role.kind === "entity") {
        const before = entities.values[role.index]!;
        const next = this.describeEntity(instance, before.parent);
        if (next === before) continue;
        if (
          next.symbolicId !== before.symbolicId ||
          next.kind !== before.kind ||
          !next.onAction !== !before.onAction ||
          !next.onActionCapture !== !before.onActionCapture
        )
          return undefined;
        entities.set(role.index, next);
        changedEntities.set(role.index, next);
      } else if (role.kind === "component") {
        const before = components.values[role.index]!;
        const cached = this.componentDescriptions.get(instance)?.description;
        const { description, structure } = this.describeComponent(
          instance,
          before.entity,
        );
        if (description === cached) continue;
        if (!cached || !sameListenerShape(cached, description))
          return undefined;
        for (const id of structure.assetIds)
          if (!described.assetIds.has(id))
            throw new Error(`Unknown asset id: ${id}`);
        const next = structure.entityReferences
          ? {
              ...description,
              fields: resolvedFields(structure, described.reference),
            }
          : description;
        if (!sameComponent(before, next)) revised = true;
        components.set(role.index, next);
        changedComponents.set(role.index, next);
      } else if (role.kind === "asset") touchedAssets.add(instance);
      else if (role.kind === "stage") touchedAssets.add(role.asset);
      else if (role.kind === "animation") {
        this.validateOnce(instance);
        const next = describeAnimation(
          instance.identity,
          instance.props as unknown as AnimationProps & {
            mailbox: AnimationMailbox;
          },
          role.parent,
          entities.values,
          described.animationAssets,
        );
        this.checkAnimation(next);
        animations.set(role.index, next);
      } else if (role.kind === "attachment") {
        const before = attachments.values[role.index]!;
        attachments.set(
          role.index,
          this.describeAttachment(instance, before.suspended),
        );
      } else {
        // Children and EntityLink: a changed placement moves entities.
        const validated = this.validated.get(instance);
        if (
          validated !== instance.props &&
          !(validated && sameDeclarationProps(validated, instance.props))
        )
          return undefined;
        this.validated.set(instance, instance.props);
      }
    }
    for (const instance of touchedAssets) {
      const role = described.roles.get(instance) as { index: number };
      const before = assets.values[role.index]!;
      const { description } = this.describeAsset(instance);
      // Other declarations resolve against asset ids and kinds.
      if (description.id !== before.id || description.kind !== before.kind)
        return undefined;
      if (
        description.version === before.version &&
        description.variant === before.variant
      )
        continue;
      // Animations resolve against an animation asset's clip.
      if (instance.type === ANIMATION_ASSET_HOST_TYPE) return undefined;
      assets.set(role.index, description);
    }
    if (
      !entities.copied &&
      !components.copied &&
      !assets.copied &&
      !animations.copied &&
      !attachments.copied
    )
      return previous;
    let resources = described.resources;
    if (assets.copied || animations.copied) {
      resources = resourceSignature(assets.values, animations.values);
      if (resources !== described.resources) revised = true;
    }
    if (revised) this.revision++;
    const description: ReactWorldDescription = {
      ...previous,
      assets: assets.values,
      animations: animations.values,
      entities: entities.values,
      components: components.values,
      links: previous.links,
      attachments: attachments.values,
      signature: String(this.revision),
      serial: this.nextSerial++,
      changes: {
        base: previous.serial!,
        entities: changedEntities,
        components: changedComponents,
      },
    };
    this.described = { ...described, description, resources };
    return description;
  }

  /**
   * Describe the whole committed host tree. Instances whose props object is
   * unchanged reuse their previous description; the signature changes only
   * when a described declaration, link, asset or animation changes.
   */
  describe(): ReactWorldDescription {
    this.touched.clear();
    this.structural = true;
    const description = this.describeAll();
    this.structural = false;
    return description;
  }

  private describeAll(): ReactWorldDescription {
    const roles = new Map<ReactWorldInstance, InstanceRole>();
    const attachments: AttachedWorldDescription[] = [];
    const pending = this.children.toReversed().map((instance) => ({
      instance,
      hidden: false,
    }));
    while (pending.length) {
      const { instance, hidden } = pending.pop()!;
      const suspended = hidden || instance.hidden;
      if (instance.type === ATTACHED_WORLD_HOST_TYPE) {
        commitCounters.described++;
        roles.set(instance, { kind: "attachment", index: attachments.length });
        attachments.push(this.describeAttachment(instance, suspended));
      } else
        for (const child of instance.children.toReversed())
          pending.push({ instance: child, hidden: suspended });
    }
    const assets: AssetDescription[] = [];
    const animationNodes: {
      instance: ReactWorldInstance;
      parent: number | undefined;
    }[] = [];
    const animationClips = new Map<
      string,
      import("@ipp/client").AnimationClipSource
    >();
    const entities: ReactEntityDescription[] = [];
    const components: ReactComponentDescription[] = [];
    const shapes: ComponentShape[] = [];
    const unresolvedLinks: (Omit<
      ReactEntityLinkDescription,
      "parent" | "before"
    > & {
      parent: ReactEntityReference | string | null;
      before: ReactEntityReference | string | null;
    })[] = [];
    const precedingChildren = new Map<
      number,
      (typeof unresolvedLinks)[number]
    >();
    const visit = (
      instance: ReactWorldInstance,
      parent?: number,
      hierarchyParent?: number,
    ): void => {
      if (instance.hidden) return;
      if (instance.type === ATTACHED_WORLD_HOST_TYPE) {
        return;
      }
      commitCounters.described++;
      const props = instance.props;
      if (instance.type === ANIMATION_HOST_TYPE) {
        this.validateOnce(instance);
        if (instance.children.length)
          throw new Error("Animation cannot contain declarations");
        roles.set(instance, {
          kind: "animation",
          index: animationNodes.length,
          parent,
        });
        animationNodes.push({ instance, parent });
        return;
      }
      if (isAsset(instance.type)) {
        const { description, clip } = this.describeAsset(instance);
        if (clip) animationClips.set(description.id, clip);
        roles.set(instance, { kind: "asset", index: assets.length });
        if (instance.type === SHADER_ASSET_HOST_TYPE)
          for (const child of instance.children)
            if (!child.hidden)
              roles.set(child, { kind: "stage", asset: instance });
        assets.push(description);
        if (instance.type === ASSET_HOST_TYPE)
          for (const child of instance.children)
            visit(child, parent, hierarchyParent);
        return;
      }
      if (isShader(instance.type)) {
        this.validateOnce(instance);
        throw new Error("Shader declarations must be children of ShaderAsset");
      }
      if (hierarchyParent !== undefined && instance.type !== ENTITY_HOST_TYPE)
        throw new Error("Children must contain Entity declarations");
      if (instance.type === CHILDREN_HOST_TYPE) {
        this.validateOnce(instance);
        if (parent === undefined)
          throw new Error("Children must be inside an Entity");
        roles.set(instance, { kind: "placement" });
        for (const child of instance.children) {
          if (child.hidden) continue;
          if (child.type !== ENTITY_HOST_TYPE)
            throw new Error("Children must contain Entity declarations");
          const preceding = precedingChildren.get(parent);
          if (preceding) preceding.before = { entity: child.identity };
          const link = {
            identity: -child.identity,
            entity: child.identity,
            parent: { entity: parent },
            before: null,
          };
          unresolvedLinks.push(link);
          precedingChildren.set(parent, link);
          visit(child, parent, parent);
        }
        return;
      }
      if (instance.type === ENTITY_HOST_TYPE) {
        roles.set(instance, { kind: "entity", index: entities.length });
        entities.push(this.describeEntity(instance, parent));
        for (const child of instance.children) visit(child, instance.identity);
        return;
      }
      if (instance.type === ENTITY_LINK_HOST_TYPE) {
        this.validateOnce(instance);
        if (parent === undefined)
          throw new Error("EntityLink must be inside an Entity");
        roles.set(instance, { kind: "placement" });
        unresolvedLinks.push({
          identity: instance.identity,
          entity: parent,
          parent: props.parent as string | bigint | null,
          before: (props.before ?? null) as string | bigint | null,
        });
        return;
      }
      if (parent === undefined) {
        this.validateOnce(instance);
        throw new Error(
          `${componentNames[instance.type]} must be inside an Entity`,
        );
      }
      const { description, structure } = this.describeComponent(
        instance,
        parent,
      );
      for (const child of instance.children) {
        if (!child.hidden && !isAsset(child.type))
          throw new Error(
            "Component children must be Asset declarations; shader stages require ShaderAsset",
          );
        visit(child, parent);
      }
      roles.set(instance, { kind: "component", index: components.length });
      components.push(description);
      shapes.push(structure);
    };
    for (const child of this.children) visit(child);

    // One node per created or adopted entity; `bindTo` only refers to one.
    // Checked before references and links, which would report the same
    // mistake less precisely.
    const declared = new Set<string>();
    for (const entity of entities) {
      if (entity.kind !== "declared") continue;
      if (declared.has(entity.symbolicId))
        throw new ReactWorldDuplicateEntityError(entity.symbolicId);
      declared.add(entity.symbolicId);
    }

    const guiActions = entities.some(
      (entity) => entity.onAction || entity.onActionCapture,
    );
    const guiEffects =
      guiActions ||
      components.some(
        (component) =>
          component.controlListeners?.onPress ||
          component.controlListeners?.onSubmit ||
          component.controlListeners?.onReject ||
          component.controlListeners?.onDiscard ||
          component.controlListeners?.onContextMenu,
      );
    const guiValues =
      (guiActions &&
        components.some((component) => component.controlListeners)) ||
      components.some(
        (component) =>
          component.controlListeners &&
          controlValueCallbackNames.some(
            (name) => component.controlListeners![name],
          ),
      );
    const guiFeedback = components.some((component) =>
      controlFeedbackCallbackNames.some(
        (name) => component.controlListeners?.[name],
      ),
    );
    if (guiEffects || guiValues || guiFeedback) this.requireOperation("gui");
    if ((guiEffects || guiFeedback) && !this.client.subscribeGuiEffects)
      throw new Error(
        "GUI callbacks require the ordinary GUI observation client",
      );
    if (guiValues) {
      if (!this.client.watchLifecycle)
        throw new Error("GUI value callbacks require lifecycle watches");
      this.requireLifecyclePublisher("GUI value callbacks");
    }

    const ids = new Set<string>();
    for (const asset of assets) {
      if (ids.has(asset.id)) throw new Error(`Duplicate asset id: ${asset.id}`);
      ids.add(asset.id);
    }
    for (const shape of shapes)
      for (const id of shape.assetIds)
        if (!ids.has(id)) throw new Error(`Unknown asset id: ${id}`);
    const symbolic = new Map<string, number[]>();
    const symbols = new Map(
      entities.map((entity) => [entity.identity, entity.symbolicId]),
    );
    for (const entity of entities) {
      const matches = symbolic.get(entity.symbolicId);
      if (matches) matches.push(entity.identity);
      else symbolic.set(entity.symbolicId, [entity.identity]);
    }
    const reference = (value: string | bigint): ReactEntityReference => {
      if (typeof value === "bigint") return value;
      const matches = symbolic.get(value);
      if (matches?.length !== 1)
        throw new Error(`Reference must identify one scene Entity: ${value}`);
      return { entity: matches[0]! };
    };
    for (let index = 0; index < components.length; index++) {
      const shape = shapes[index]!;
      if (shape.entityReferences)
        components[index] = {
          ...components[index]!,
          fields: resolvedFields(shape, reference),
        };
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
    for (const animation of animations) this.checkAnimation(animation);
    // Declarations sharing a symbolic id reach one entity, so at most one of
    // them may place it.
    const parented = new Set<string>();
    const links = unresolvedLinks.map((link): ReactEntityLinkDescription => {
      const symbol = symbols.get(link.entity)!;
      if (parented.has(symbol))
        throw new Error(
          "Children or EntityLink conflicts with another link declaration",
        );
      parented.add(symbol);
      return {
        ...link,
        parent:
          typeof link.parent === "string"
            ? reference(link.parent)
            : link.parent,
        before:
          typeof link.before === "string"
            ? reference(link.before)
            : link.before,
      };
    });

    const resources = resourceSignature(assets, animations);
    const previous = this.described;
    if (
      !previous ||
      previous.resources !== resources ||
      !sameEach(previous.description.entities, entities, sameEntity) ||
      !sameEach(previous.description.components, components, sameComponent) ||
      !sameEach(previous.description.links, links, sameLink)
    )
      this.revision++;
    const description: ReactWorldDescription = {
      guiActions,
      guiEffects,
      guiFeedback,
      assets,
      animations,
      entities,
      components,
      links,
      attachments,
      signature: String(this.revision),
      serial: this.nextSerial++,
    };
    this.described = {
      description,
      resources,
      roles,
      assetIds: ids,
      animationAssets,
      reference,
    };
    return description;
  }
}

/**
 * The asset and animation declarations as compared between descriptions.
 * Preserves NaN, infinities and -0 so encoder-rejected authored values cannot
 * compare equal to a later corrected declaration.
 */
function resourceSignature(
  assets: readonly AssetDescription[],
  animations: readonly AnimationDescription[],
): string {
  return JSON.stringify([
    assets.map(({ bytes, signature, ...asset }) => asset),
    animations.map(({ mailbox, onPlaybackEvent, ...description }) =>
      animationSignature(description),
    ),
  ]);
}

/** An array copied on its first change. */
class Copied<T> {
  copied = false;

  constructor(public values: readonly T[]) {}

  set(index: number, value: T): void {
    if (!this.copied) {
      this.values = [...this.values];
      this.copied = true;
    }
    (this.values as T[])[index] = value;
  }
}

/**
 * Whether two descriptions of one component declaration have the same
 * listeners and ref present, which decide what a root observes and checks.
 */
function sameListenerShape(
  left: ReactComponentDescription,
  right: ReactComponentDescription,
): boolean {
  if (!left.controlRef !== !right.controlRef) return false;
  if (!left.controlListeners || !right.controlListeners)
    return left.controlListeners === right.controlListeners;
  const keys = Object.keys(right.controlListeners);
  return (
    keys.length === Object.keys(left.controlListeners).length &&
    keys.every((key) => Object.hasOwn(left.controlListeners!, key))
  );
}

/** Declaration inputs derived from one component's props. */
interface ComponentShape {
  readonly component: number;
  readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  readonly properties: Readonly<Record<string, DynamicValue>> | undefined;
  readonly assetIds: readonly string[];
  readonly entityReferences: boolean;
  resolved?: {
    readonly key: string;
    readonly fields: ReadonlyMap<number, ReactWorldFieldValue>;
  };
}

/**
 * Whether a props update leaves every declared value unchanged. Callbacks and
 * callback refs are listeners rather than declared values; byte arrays always
 * compare by content because callers may reuse and mutate them. Plain arrays
 * and objects, such as vectors and binding lists that renders pass as fresh
 * literals, compare by value.
 */
function sameDeclarationProps(
  previous: ReactWorldElementProps,
  next: ReactWorldElementProps,
): boolean {
  const keys = Object.keys(next);
  if (keys.length !== Object.keys(previous).length) return false;
  for (const key of keys) {
    if (key === "children") continue;
    if (!Object.hasOwn(previous, key)) return false;
    const before = previous[key];
    const after = next[key];
    if (Object.is(before, after)) {
      if (after instanceof Uint8Array) return false;
      continue;
    }
    if (typeof before === "function" && typeof after === "function") continue;
    if (!samePlainData(before, after, 0)) return false;
  }
  return true;
}

/** Whether two plain arrays or objects hold equal primitive values. */
function samePlainData(left: unknown, right: unknown, depth: number): boolean {
  if (Object.is(left, right)) return !(right instanceof Uint8Array);
  if (
    depth > 4 ||
    typeof left !== "object" ||
    typeof right !== "object" ||
    left === null ||
    right === null
  )
    return false;
  if (Array.isArray(left)) {
    if (!Array.isArray(right) || left.length !== right.length) return false;
    for (let index = 0; index < left.length; index++)
      if (!samePlainData(left[index], right[index], depth + 1)) return false;
    return true;
  }
  if (
    Object.getPrototypeOf(left) !== Object.prototype ||
    Object.getPrototypeOf(right) !== Object.prototype
  )
    return false;
  const keys = Object.keys(right);
  if (keys.length !== Object.keys(left).length) return false;
  for (const key of keys)
    if (
      !Object.hasOwn(left, key) ||
      !samePlainData(
        (left as Record<string, unknown>)[key],
        (right as Record<string, unknown>)[key],
        depth + 1,
      )
    )
      return false;
  return true;
}

function sameListeners(
  left: GuiControlListeners | undefined,
  right: GuiControlListeners | undefined,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  const keys = Object.keys(right) as (keyof GuiControlListeners)[];
  return (
    keys.length === Object.keys(left).length &&
    keys.every((key) => left[key] === right[key])
  );
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index++)
    if (left[index] !== right[index]) return false;
  return true;
}

function sameFieldValue(
  left: ReactWorldFieldValue,
  right: ReactWorldFieldValue,
): boolean {
  if (left.kind !== right.kind) return false;
  if (
    (left.kind === "bytes" || left.kind === "rows") &&
    (right.kind === "bytes" || right.kind === "rows")
  )
    return sameBytes(left.value, right.value);
  if (
    left.kind === "asset" ||
    left.kind === "row-asset" ||
    left.kind === "entity-reference"
  )
    return Object.is(left.value, (right as typeof left).value);
  return fieldIdentity(left) === fieldIdentity(right as DeclarationFieldValue);
}

function sameShape(left: ComponentShape, right: ComponentShape): boolean {
  if (
    left.component !== right.component ||
    left.fields.size !== right.fields.size
  )
    return false;
  for (const [offset, value] of right.fields) {
    const previous = left.fields.get(offset);
    if (!previous || !sameFieldValue(previous, value)) return false;
  }
  if (!left.properties || !right.properties)
    return left.properties === right.properties;
  return (
    attachmentIdentity(left.properties) === attachmentIdentity(right.properties)
  );
}

/** Symbolic entity fields resolved against this description's entities. */
function resolvedFields(
  shape: ComponentShape,
  reference: (value: string | bigint) => ReactEntityReference,
): ReadonlyMap<number, ReactWorldFieldValue> {
  const targets = new Map<number, ReactEntityReference>();
  for (const [offset, value] of shape.fields)
    if (value.kind === "entity-reference" && typeof value.value === "string")
      targets.set(offset, reference(value.value));
  const key = [...targets]
    .map(([offset, target]) =>
      typeof target === "bigint"
        ? `${offset}:${target}n`
        : `${offset}:${target.entity}`,
    )
    .join();
  if (shape.resolved?.key === key) return shape.resolved.fields;
  const fields = new Map(shape.fields);
  for (const [offset, target] of targets)
    fields.set(offset, { kind: "entity-reference", value: target });
  shape.resolved = { key, fields };
  return fields;
}

function sameEach<T>(
  left: readonly T[],
  right: readonly T[],
  same: (left: T, right: T) => boolean,
): boolean {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index++)
    if (!same(left[index]!, right[index]!)) return false;
  return true;
}

function sameEntity(
  left: ReactEntityDescription,
  right: ReactEntityDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.symbolicId === right.symbolicId &&
    left.kind === right.kind &&
    left.parent === right.parent
  );
}

function sameComponent(
  left: ReactComponentDescription,
  right: ReactComponentDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.entity === right.entity &&
    left.component === right.component &&
    left.fields === right.fields &&
    left.properties === right.properties
  );
}

function sameReference(
  left: ReactEntityReference | null,
  right: ReactEntityReference | null,
): boolean {
  if (left === null || right === null || typeof left === "bigint")
    return left === right;
  return typeof right === "object" && left.entity === right.entity;
}

function sameLink(
  left: ReactEntityLinkDescription,
  right: ReactEntityLinkDescription,
): boolean {
  return (
    left.identity === right.identity &&
    left.entity === right.entity &&
    sameReference(left.parent, right.parent) &&
    sameReference(left.before, right.before)
  );
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
