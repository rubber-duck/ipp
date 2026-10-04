/** Connection authority and semantic exports; source names alone never grant reads. */
import {
  type BulkReadClient,
  type BulkReadDescriptor,
  type BulkReadOptions,
  readBulkReference,
} from "./bulk-reads.js";
import type { HostWireReader, HostWireWriter } from "./host-protocol.js";
import type { ClientAssetSource } from "./types.js";

export type AssetExportFormat =
  | "mesh-v3"
  | "texture-v3"
  | "skeleton-v1"
  | "pose-v1"
  | "skin-v1"
  | "shader-v3"
  | "animation-v4"
  | "geometry-v1"
  | "particle-cache-v1"
  | "expression-v1";
export type AssetReadRepresentation = "original" | "cpu" | "gpu";
export type AssetReadSelection =
  | { readonly representation: "original" }
  | {
      readonly representation: "cpu" | "gpu";
      readonly format: AssetExportFormat;
    };

export interface AssetReadCapability {
  readonly connection: bigint;
  readonly grant: bigint;
}

export interface AuthorizedAssetSource {
  readonly capability: AssetReadCapability;
  readonly source: ClientAssetSource;
  readonly original: boolean;
  /** Complete retained CPU semantic representations available when queried. */
  readonly cpu: readonly AssetExportFormat[];
  /** Explicit supported GPU encodings, independent of temporary residency loss. */
  readonly gpu: readonly AssetExportFormat[];
}

export interface AssetExportRead {
  readonly read: BulkReadDescriptor;
  readonly representation: AssetReadRepresentation;
  readonly format?: AssetExportFormat | undefined;
}

const formats: Readonly<Record<AssetExportFormat, string>> = {
  "mesh-v3": "ASSET_FORMAT_MESH_V3",
  "texture-v3": "ASSET_FORMAT_TEXTURE_V3",
  "skeleton-v1": "ASSET_FORMAT_SKELETON_V1",
  "pose-v1": "ASSET_FORMAT_POSE_V1",
  "skin-v1": "ASSET_FORMAT_SKIN_V1",
  "shader-v3": "ASSET_FORMAT_SHADER_V3",
  "animation-v4": "ASSET_FORMAT_ANIMATION_V4",
  "geometry-v1": "ASSET_FORMAT_GEOMETRY_V1",
  "particle-cache-v1": "ASSET_FORMAT_PARTICLE_CACHE_V1",
  "expression-v1": "ASSET_FORMAT_EXPRESSION_V1",
};

export class HostAssets {
  constructor(
    private readonly request: (
      tag: number,
      encode: (writer: HostWireWriter) => void,
    ) => Promise<HostWireReader>,
    private readonly tag: (name: string) => number,
    private readonly reads: BulkReadClient,
  ) {}

  private format(tag: number): AssetExportFormat {
    for (const [format, name] of Object.entries(formats))
      if (this.tag(name) === tag) return format as AssetExportFormat;
    throw new Error("Unknown asset semantic format");
  }

  private writeCapability(
    writer: HostWireWriter,
    capability: AssetReadCapability,
  ): void {
    if (capability.connection <= 0n || capability.grant <= 0n)
      throw new RangeError("Invalid asset read capability");
    writer.u64(capability.connection);
    writer.u64(capability.grant);
  }

  private async operation(
    encode: (writer: HostWireWriter) => void,
  ): Promise<HostWireReader> {
    const reader = await this.request(
      this.tag("HOST_REQUEST_ASSET_EXPORT"),
      encode,
    );
    if (reader.u8() !== this.tag("HOST_RESPONSE_ASSET_EXPORT"))
      throw new Error("Unexpected asset export response");
    return reader;
  }

  /** Resolve only an existing own/shared capability or explicitly public Host policy. */
  async find(source: ClientAssetSource): Promise<AuthorizedAssetSource> {
    const reader = await this.operation((writer) => {
      writer.u8(this.tag("ASSET_EXPORT_FIND"));
      writer.u16(source.kind);
      writer.string(source.source);
      writer.u32(source.variant ?? 0);
    });
    if (reader.u8() !== this.tag("ASSET_EXPORT_CAPABILITY"))
      throw new Error("Unexpected asset capability response");
    const capability = { connection: reader.u64(), grant: reader.u64() };
    if (capability.connection === 0n || capability.grant === 0n)
      throw new Error("Invalid asset capability response");
    const grantedSource = {
      kind: reader.u16(),
      source: reader.string(),
      variant: reader.u32(),
    };
    const original = reader.boolean();
    const readFormats = () => {
      const count = reader.u32();
      if (count > 10) throw new Error("Invalid asset format list");
      return Array.from({ length: count }, () => this.format(reader.u8()));
    };
    const cpu = readFormats();
    const gpu = readFormats();
    reader.end();
    return { capability, source: grantedSource, original, cpu, gpu };
  }

  /** Return a delivery lease only after a complete private semantic export succeeds. */
  async read(
    capability: AssetReadCapability,
    selection: AssetReadSelection,
  ): Promise<AssetExportRead> {
    const reader = await this.operation((writer) => {
      writer.u8(this.tag("ASSET_EXPORT_READ"));
      this.writeCapability(writer, capability);
      writer.u8(
        this.tag(
          `ASSET_REPRESENTATION_${selection.representation.toUpperCase()}`,
        ),
      );
      writer.u8(selection.representation === "original" ? 0 : 1);
      if (selection.representation !== "original")
        writer.u8(this.tag(formats[selection.format]));
    });
    if (reader.u8() !== this.tag("ASSET_EXPORT_OPENED"))
      throw new Error("Unexpected asset export read response");
    const reference = readBulkReference(reader);
    const length = reader.boolean() ? reader.u64() : undefined;
    const tag = reader.u8();
    const representation = (["original", "cpu", "gpu"] as const).find(
      (value) =>
        this.tag(`ASSET_REPRESENTATION_${value.toUpperCase()}`) === tag,
    );
    if (representation === undefined)
      throw new Error("Unknown asset representation");
    const format = reader.boolean() ? this.format(reader.u8()) : undefined;
    reader.end();
    if (
      representation !== selection.representation ||
      format !==
        (selection.representation === "original" ? undefined : selection.format)
    )
      throw new Error("Asset export representation mismatch");
    return { read: { reference, length }, representation, format };
  }

  /** Read complete bytes through the common bounded data plane and acknowledge EOF.
   * The signal cancels byte delivery after publication; it does not revoke the grant
   * or cancel an encoding already accepted by the Host. Revoke the capability to
   * cancel all its pending reads/exports. An already aborted signal starts no work.
   */
  async readAll(
    capability: AssetReadCapability,
    selection: AssetReadSelection,
    options: BulkReadOptions = {},
  ): Promise<Uint8Array<ArrayBuffer>> {
    if (options.signal?.aborted)
      throw (
        options.signal.reason ??
        new DOMException("Asset read aborted", "AbortError")
      );
    const output = await this.read(capability, selection);
    return this.reads.readAll(output.read, options);
  }

  /** Revoke this connection grant; detached completed typed output remains readable. */
  async revoke(capability: AssetReadCapability): Promise<void> {
    const reader = await this.operation((writer) => {
      writer.u8(this.tag("ASSET_EXPORT_REVOKE"));
      this.writeCapability(writer, capability);
    });
    if (reader.u8() !== this.tag("ASSET_EXPORT_REVOKED"))
      throw new Error("Unexpected asset revocation response");
    reader.end();
  }
}
