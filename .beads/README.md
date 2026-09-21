# IPP Coordination Setup

Follow the [coordination workflow](../docs/development/coordination.md) and [agent guide](../AGENTS.md). Review inherited tasks against current architecture before claiming work.

Use one project-local Dolt server shared by linked Git worktrees. Source Git tracks configuration and guidance; task data remains in Dolt. Start the server explicitly. Do not initialize another database or commit raw database files. Task history and source Git are checkpointed and published separately.
