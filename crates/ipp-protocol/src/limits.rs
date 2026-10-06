//! Message, page and collection bounds shared by the protocol lanes.

/// Maximum complete application message, before decoding or allocation.
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;

/// Most lifecycle publications in one response; a session's drained observations span as
/// many responses as they need.
pub const MAX_LIFECYCLE_PUBLICATIONS: usize = 128;

/// Maximum commands in one batch page; larger batches span several pages.
///
/// 1024 commands carry about 78 thirteen-command GUI rows, so ordinary edits
/// travel as one page. A full page's command slots (1024 × at most
/// [`ipp_core::MAX_COMMAND_INLINE_BYTES`] = 128 KiB) fit one recycled World
/// command buffer, which the Host decodes pages into.
pub const COMMAND_PAGE_COMMANDS: usize = 1024;

const _: () = assert!(COMMAND_PAGE_COMMANDS <= ipp_core::RECYCLED_COMMAND_BUFFER_COMMANDS);

/// Maximum encoded bytes of one batch page message.
///
/// Measured commands encode to 41 bytes on average for GUI rows and 89 for
/// Blender scenes, but URL-bearing mesh instances take about 150 bytes, so
/// 1024 of them would exceed 128 KiB. 256 KiB lets pages of commands up to
/// about 250 bytes reach [`COMMAND_PAGE_COMMANDS`], so the command limit rather
/// than the byte limit decides page size, while staying a quarter of
/// [`MAX_MESSAGE_BYTES`].
pub const COMMAND_PAGE_BYTES: usize = 256 * 1024;

/// Most entity aliases and symbol reports one batch outcome carries for one
/// logical batch.
///
/// A batch is answered once, on its final page, with every alias it defined and
/// every symbol and handle its symbolic references resolved to, so this bounds
/// the whole batch; the Host rejects a batch that defines more aliases or can
/// report more symbols before it applies. Symbol text counts against the reply's
/// message size at the same admission. Each alias report encodes to 12 bytes,
/// so a full alias list takes 384 KiB, within [`MAX_MESSAGE_BYTES`]; it is
/// about three times the entities of the maintained 40,064-command Blender
/// stress import, which the Host answers in one reply. Commands that define
/// no alias add only their symbol reports and applied effects to the reply.
pub const BATCH_OUTCOME_ALIASES: usize = 32_768;

const _: () = assert!(COMMAND_PAGE_COMMANDS <= BATCH_OUTCOME_ALIASES);

/// Most applied effects one batch outcome carries: as many of the smallest, an
/// adoption report of [`crate::world::attachment_receipts::ADOPTED_EFFECT_BYTES`], as one
/// message holds.
///
/// The Host reserves each effect's encoded size in the batch's reply before the
/// effect can apply (adoption reports with the batch, attachment effects per
/// operation), so the message size rather than this count decides how many
/// effects one outcome reports.
pub const BATCH_OUTCOME_EFFECTS: usize =
    MAX_MESSAGE_BYTES / crate::world::attachment_receipts::ADOPTED_EFFECT_BYTES;

/// Largest ordinary text or byte field: names, symbolic identities, sources, reasons,
/// dynamic property values and transfer chunks.
///
/// Protocol framing limit. 64 KiB keeps any single field a small share of
/// [`MAX_MESSAGE_BYTES`], so one message still carries several; encoding a longer field
/// fails and decoding one reports a limit error.
pub const MAX_FIELD_BYTES: usize = 65_536;

/// Classes one entity's metadata carries on the wire.
///
/// Classes are short authoring labels; 256 is far beyond any maintained scene while keeping
/// metadata a bounded share of a command page. Longer lists fail to encode or decode.
pub const MAX_METADATA_CLASSES: usize = 256;

/// Field values one `InsertComponent` command writes.
///
/// Matches the widest compiled component, whose field count stays well under 256; a
/// longer list fails to encode or decode.
pub const MAX_INSERT_FIELDS: usize = 256;

/// Records of one kind in one inspection or entity-tree page, and the largest page a
/// query may request.
///
/// Inspection pages; a query continues from the returned cursor. 256 records of the
/// largest kind stay within [`MAX_MESSAGE_BYTES`]; a larger requested page is malformed.
pub const INSPECTION_PAGE_RECORDS: usize = 256;

/// Components one inspected entity reports in a page.
///
/// Bounded by the compiled component registry, well under 256; a longer list fails to
/// encode or decode.
pub const MAX_INSPECTED_COMPONENTS: usize = 256;

/// Fields one inspected component reports: at most one per 16-bit schema offset.
pub const MAX_INSPECTED_FIELDS: usize = 65_536;

/// Deepest descendant level an entity-tree query may request below its root.
///
/// A query page never descends further; deeper descendants are reached by querying from
/// a deeper root. Larger requests are malformed.
pub const MAX_ENTITY_TREE_DEPTH: u16 = 64;

/// Resources one resource event reports; the Host splits larger sets across events.
pub const MAX_RESOURCE_EVENT_RECORDS: usize = 128;

/// Playback events one response reports; the Host splits larger sets across responses.
pub const MAX_PLAYBACK_EVENTS: usize = 1024;

/// UTF-8 bytes of a batch-abort or runtime-failure message; longer diagnostics are
/// truncated by their producer and refused by the codec.
pub const MAX_FAILURE_MESSAGE_BYTES: usize = 2048;

/// Property offsets or joint indices one animation target names.
///
/// 4096 covers every field of the widest component and every joint of a skeleton many
/// times over; a longer list fails to encode or decode.
pub const MAX_ANIMATION_TARGET_INDICES: usize = 4096;
