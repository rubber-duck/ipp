# Implementation Strategies

[Architecture](../architecture.md) owns design; these documents own approach and validation. Beads owns tasks, dependencies, acceptance and handoffs. APIs, formats and algorithms belong with source.

| Strategy | Focus |
| --- | --- |
| [ECS and serialization](ecs-serialization.md) | Storage, generated access, GUI values, lifecycle and persistence |
| [React reconciliation](react-reconciler.md) | Scene/GUI declarations, acknowledgement and cleanup |
| [Runtime, assets and rendering](runtime-and-rendering.md) | Hosts, evaluation, resources, GUI and GL integration |
| [Blender integration](blender-integration.md) | Export translation, local transport and real Blender/viewer evidence |

Each strategy identifies real participants, fixtures, observations and reusable environment drivers under the [testing policy](../development/integration-testing.md). Keep execution queues, status and detailed cases out of plans. Follow the [workflow](../development/workflow.md#architecture-and-strategy) for architectural changes.
