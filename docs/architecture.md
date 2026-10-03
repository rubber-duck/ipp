# IPP — Architecture Overview

IPP (Interactive Presentation Platform) is a headless Rust runtime for interactive 3D worlds. Clients author state; platform hosts supply time, I/O and presentation. Web is the first viewer; native embedding and Blender authoring are also in scope.

## System boundaries

```mermaid
flowchart TD
    clients["Applications / authoring"] --> sdk["Generated SDK"]
    sdk -->|"World sessions / ordered batches"| world["Worlds: headless evaluation"]
    world -->|"Outcomes and events"| sdk
    host["Host: clock / lifecycle"] -->|"Schedules attachment graph"| world
    clients -->|"Owned asset data"| assets["Host asset services"]
    assets -->|"Shared resources"| world
    clients -->|"Dataset protocol / bulk updates"| data["Host Data Service"]
    data -->|"Typed sources / shared retention"| world
    world -->|"Completed outputs"| render["RenderService: composition / GPU"]
    host -->|"Root output / context / surface"| render
```

## Design principles

One mutation owner, one stored value per component field, stable storage, demand-driven resources, target-correct contracts and one evaluation owner per operation. Uniform Worlds compose through explicit attachments; GUI is one selection of ordinary entity systems. The owning topics below define these contracts; architecture describes accepted direction on the way to v1, not implementation status.

## Architecture topics

| Topic | Owns |
| --- | --- |
| [Runtime](architecture/runtime.md) | Host/World ownership, state, lifetime and evaluation order |
| [Protocol and schema](architecture/protocol-and-schema.md) | Sessions, generated contracts and persistence |
| [Assets](architecture/assets.md) | Identity, demand, I/O, loading and recovery |
| [Data](architecture/data.md) | Typed sources, data bindings, updates and shared retention |
| [Rendering](architecture/rendering.md) | GPU boundary, materials, cameras and interaction |
| [GUI](architecture/gui.md) | Canvas content, layout, local controls and platform input |
| [Authoring](architecture/authoring.md) | React and Blender integration boundaries |
| [Rust workspace](architecture/rust-workspace.md) | Crates, capabilities and dependencies |

[Strategies](plans/README.md) explain implementation approach. Source and dedicated reference guides own APIs, algorithms and formats; READMEs introduce intent and boundaries. The [build guide](development/building.md) records current capabilities. Beads tracks executable work under the [development workflow](development/workflow.md).
