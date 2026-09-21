# IPP — Interactive Presentation Platform

IPP is a headless Rust runtime for interactive 3D scenes, with generated client SDKs and optional rendering shared across WebGL 2 and native GLES 3.

Clients author state; the core owns evaluation and lifecycle, and platform Hosts supply clocks, I/O and presentation. Generated TypeScript clients connect to native or browser Hosts. [React](packages/ipp-react/README.md) provides declarative scene composition; [Blender](integrations/blender/README.md) supplies authoring and export.

Before stabilization, APIs and formats may change directly. Clients and fixtures must match the receiving runtime build.

- [Architecture](docs/architecture.md) is the source of truth for accepted design.
- [Implementation strategies](docs/plans/README.md) describe direction and maintained integration harnesses.
- [Build guide](docs/development/building.md) covers setup, current scope and validation.
- [Scene gallery](examples/world-gallery/README.md) and [headless client](examples/headless-client/README.md) are runnable entry points.
- [Agent guide](AGENTS.md) and [development workflow](docs/development/workflow.md) define architecture review, Beads coordination, and validation.

With Rustup, Python from `.python-version`, and Node from `.node-version` installed:

```sh
python tools/ipp.py setup node
python tools/ipp.py setup rust
python tools/ipp.py test native
```

The build guide covers additional browser and Blender prerequisites. Beads/Dolt is needed for task coordination, not compilation.
