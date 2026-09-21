# Built-in resources

The optional `builtin-assets` capability supplies procedural `ipp://` meshes and textures through ordinary asset loading. Generation performs no external I/O or GPU work. Private geometry visualization reuses shape construction without enabling public built-in sources.

Recipes describe immutable local-space content. Source identity remains literal: equivalent parameter orderings can generate the same geometry without becoming the same resource. Recovery reproduces content within the selected build; cross-build byte identity is not promised.

[Recipe parsing](mod.rs) validates parameters, and [resource generation](resources.rs) owns the supported recipe names, dimensions, colors and encoding. [Shape implementations](shapes) own geometric restrictions. Mesh lengths use metres; outline stroke is a physical diameter. [Rig fixtures](rig.rs) additionally require `skeletal-animation`; see the [rig guide](../../../../../../docs/development/skeletal-skinning.md#built-in-fixture).

To export an ordinary mesh file from the repository root:

```sh
cargo run -q -p ipp-core --features builtin-assets --example export_builtin --locked -- \
  mesh 'ipp://mesh/cube?width=2&height=2&length=2' > /tmp/cube.mesh
```

The [export example](../../../../examples/export_builtin.rs) also accepts texture recipes. The [native rendering example](../../../../../ipp-render-gl/examples/smoke/README.md) shows texture and shape fixture usage. Ownership follows the [asset architecture](../../../../../../docs/architecture/assets.md).
