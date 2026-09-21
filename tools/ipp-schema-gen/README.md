# Target contract generation

The generator turns an executed target's verified binary export into matching TypeScript contracts and codecs. It never links a native core to infer another target's layout. The [protocol architecture](../../docs/architecture/protocol-and-schema.md#build-compatibility) owns compatibility; the [generation strategy](../../docs/plans/ecs-serialization.md#target-contracts-and-generation) describes validation.

Exports describe the compiled registry, field access and creation defaults, wire/asset formats and selected capabilities. The generator checks the compatibility hash before interpreting those declarations. Hashes identify matching builds, not authenticity. Target layouts, wire encoding and GPU packing remain distinct.

Generate each client from its actual target export with the same capabilities as the final Host. Disabled capabilities omit their declarations/codecs; internal component fields remain unavailable for generic authoring. Dynamic properties are an opt-in capability of a compiled component, not runtime component registration. Derives currently support named, non-generic `repr(C)` structs; supported field kinds and restrictions live in the [derive implementation](../ipp-schema-derive/src/component_derive.rs).

## Build integration

Emit generated files into target-specific artifact directories. Update maintained templates and regenerate clients instead of hand-editing generated output. After obtaining an executed target export, the CLI is:

```sh
cargo run -p ipp-schema-gen --locked -- target/runtime.contract target/generated.ts
```

The maintained [assembler](../../packages/ipp-client/tools/assemble.mjs) copies target-independent client support beside generated output and assembles browser hosts with matching capabilities. Its module list and option names are authoritative. The [client guide](../../packages/ipp-client/README.md) owns connection and authoring usage.

Start with [export reading and validation](src/export_reader.rs), [wire contracts](src/wire_contract.rs) and [TypeScript generation](src/typescript.rs). The maintained [codec template](src/codec.template.ts) includes capability-specific helpers. [Target verification](tests/target-contract.mjs) executes native/WASM contracts and checks typed access and actual layout differences rather than assuming portability.

`python tools/ipp.py test contracts` runs maintained target verification; `python tools/ipp.py test client` checks generated ownership/correlation behavior. `python tools/ipp.py test browser` exercises final worker/WASM builds with generated clients. Contract checks alone do not establish completed-frame rendering evidence. Export tooling is available only in schema-export builds; production Hosts retain their compatibility hash and required dispatch.
