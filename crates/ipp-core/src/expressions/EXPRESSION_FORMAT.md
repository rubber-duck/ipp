# Expression Declaration Format

[Codec and limits](codec.rs) · [Logical declarations](declaration.rs) · [Preparation](preparation.rs) · [Independent fixtures](codec_tests.rs) · [Runtime boundary](../../../../docs/architecture/runtime.md#pure-expression-evaluation)

`ExpressionDeclaration::encode` and `ExpressionDeclaration::decode` share one complete portable payload for immutable definition assets and persisted drivers. Consumers own asset registration, binding resolution and any enclosing framing; an enclosing field must supply the exact payload slice. This codec stores the authored input slots, graph topology and output, including unused nodes and shared/forward references. Prepared instructions, scratch values and consumer identities are reconstructed separately.

## Identity and fields

All integers are fixed-width little-endian. There is no alignment, padding, native `usize`, target component layout or implicit numeric conversion. Version denotes format identity, independent of source revisions and asset type IDs. Unknown versions and tags are rejected; trailing bytes are rejected.

| Field       | Encoding                                 |
| ----------- | ---------------------------------------- |
| Magic       | Four ASCII bytes `IPPE`                  |
| Version     | `u32`, currently `1`                     |
| Input count | `u32`                                    |
| Node count  | `u32`                                    |
| Output      | `u32` node index                         |
| Inputs      | Input count records, in slot order       |
| Nodes       | Node count records, in declaration order |

An input is a string followed by a `u8` type tag. A string is a `u32` UTF-8 byte length followed by exactly those bytes, without a terminator. Input names must be nonempty and unique; text constants may be empty and may contain null bytes. Each index is a zero-based `u32`; `Input` refers to an input slot and all other references refer to nodes.

| Node tag | Node     | Fields following the tag                   |
| -------- | -------- | ------------------------------------------ |
| `0`      | Input    | Slot index                                 |
| `1`      | Constant | Type tag and typed constant payload        |
| `2`      | Unary    | `u8` operator tag, operand index           |
| `3`      | Binary   | `u8` operator tag, left index, right index |
| `4`      | Clamp    | Value index, minimum index, maximum index  |
| `5`      | Ternary  | Condition index, then index, else index    |
| `6`      | Fallback | Value index, replacement index             |

Unary tags are `0` Negate, `1` Absolute, `2` Not and `3` Length. Binary tags are `0` Add, `1` Subtract, `2` Multiply, `3` Divide, `4` Minimum, `5` Maximum, `6` Equal, `7` Less, `8` Greater, `9` And and `10` Or. The [declaration API](declaration.rs) and [evaluator contract](mod.rs) own their semantics.

## Exact core values

Numeric and boolean tags/payloads match the core's [canonical dynamic value representation](../components/dynamic_properties/value.rs). Text has its own length-prefixed representation here because the canonical dynamic decoder excludes row-only text. Asset values are unsupported. Tags `0`, `11`, `12` and all unlisted tags are rejected.

| Type tag | Type | Constant payload |
| --- | --- | --- |
| `1` | F32 | One finite IEEE 754 binary32, preserving its bits |
| `2` | I32 | One signed two's-complement `i32` |
| `3` | U32 | One `u32` |
| `4` | Bool | One `u32`: exactly `0` or `1` |
| `5`, `6`, `7` | Vec2, Vec3, Vec4 | 2, 3 or 4 finite binary32 lanes |
| `8`, `9`, `10` | Mat2, Mat3, Mat4 | 4, 9 or 16 finite binary32 lanes in column-major order |
| `13` | Text | Length-prefixed UTF-8 string |

Integer constants retain all 32 bits and never pass through `f32`. Float/vector/matrix constants reject NaN and infinities. Negative zero retains its encoded bits.

## Bounds and validation

Public constants in [codec.rs](codec.rs) define the format bounds: at most 16 MiB for the complete payload, at most 65,536 inputs and 65,536 nodes independently, and at most 1 MiB of UTF-8 bytes per name or text constant. Aggregate string bytes also count toward the complete payload bound. These bounds are identical on native and WASM targets.

Decoding checks the complete size and bounded counts, then checks the minimum possible record footprint before reserving input/node buffers. Every field uses checked slice access; string length and UTF-8 are checked before copying. Input/node buffers and name strings use fallible reservations. Text constants use the standard library's bounded `Arc<str>` allocation; shared semantic validation also uses bounded standard-library storage. Allocation failure in those standard-library operations follows Rust's allocator behavior rather than returning `AllocationFailed`.

The decoder rejects out-of-range output, input and node references. The same semantic validator used by preparation checks every declared node, including unused nodes: names, supported types, constants, operand/branch types, cycles, dependency height and expanded output instruction count. Existing limits are 256 nodes per dependency path and 65,536 expanded instructions. Validation does not compile a plan, expand shared graph occurrences or allocate scratch. Encoding measures and checks its byte budget before semantic validation and output allocation; invalid declarations are refused instead of producing bytes a decoder cannot accept.

The codec provides declaration validation and portability evidence only. Asset loading/registration, driver persistence, generated clients and real transport scenarios belong to its consumers.
