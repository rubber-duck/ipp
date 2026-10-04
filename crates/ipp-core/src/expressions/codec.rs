//! Portable logical declarations; see [EXPRESSION_FORMAT.md](EXPRESSION_FORMAT.md).
//! Asset registration and consumer persistence wrap these same complete payloads.

use super::preparation::validate_declaration;
use super::*;
use std::fmt;

/// Stable expression declaration format identity, independent of asset type IDs.
pub const EXPRESSION_FORMAT_MAGIC: [u8; 4] = *b"IPPE";
/// Current format version, encoded as a little-endian `u32`.
pub const EXPRESSION_FORMAT_VERSION: u32 = 1;
/// Maximum complete encoded payload size (16 MiB), checked before decoding.
pub const EXPRESSION_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Maximum inputs and maximum nodes, each independently bounded.
pub const EXPRESSION_MAX_ITEMS: u32 = super::preparation::MAX_ITEMS as u32;
/// Maximum UTF-8 bytes in each input name or text constant (1 MiB).
pub const EXPRESSION_MAX_STRING_BYTES: usize = 1024 * 1024;

/// Refused portable declaration, distinct from per-sample evaluation invalidity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionCodecError {
    /// Incorrect four-byte format magic.
    InvalidHeader,
    /// The payload uses an unsupported format version.
    UnsupportedVersion,
    /// A field extends beyond the supplied payload.
    Truncated,
    /// Bytes remain after the single complete declaration.
    TrailingBytes,
    /// A byte length or item count exceeds the format's bounds.
    LimitExceeded,
    /// An unknown node, operator or unsupported core type tag was supplied.
    InvalidTag,
    /// An input name or text constant contains invalid UTF-8.
    InvalidUtf8,
    /// A constant has an invalid representation (nonfinite float or noncanonical boolean).
    InvalidValue,
    /// Existing expression preparation rejects the logical graph or its types.
    InvalidDeclaration(ExpressionPrepareError),
    /// A checked buffer reservation failed.
    AllocationFailed,
}

impl fmt::Display for ExpressionCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDeclaration(reason) => {
                write!(f, "Invalid expression declaration: {reason:?}")
            }
            _ => write!(f, "Expression codec: {self:?}"),
        }
    }
}

impl std::error::Error for ExpressionCodecError {}

impl From<ExpressionPrepareError> for ExpressionCodecError {
    fn from(value: ExpressionPrepareError) -> Self {
        Self::InvalidDeclaration(value)
    }
}

impl ExpressionDeclaration {
    /// Validate and encode this logical graph as one complete IPPE v1 payload.
    /// No prepared instructions, scratch, consumer bindings or runtime identities are stored.
    pub fn encode(&self) -> Result<Vec<u8>, ExpressionCodecError> {
        // Measure first: oversized authored strings never enter validation or an output buffer.
        let mut measure = Writer::default();
        encode_declaration(&mut measure, self)?;
        validate_declaration(self)?;

        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(measure.length)
            .map_err(|_| ExpressionCodecError::AllocationFailed)?;
        let mut writer = Writer {
            bytes: Some(&mut bytes),
            length: 0,
        };
        encode_declaration(&mut writer, self)?;
        Ok(bytes)
    }

    /// Decode exactly one bounded IPPE v1 declaration, rejecting trailing bytes.
    /// All nodes (including unused nodes) undergo the same semantic validation as preparation.
    /// The caller prepares the result separately; decoding never builds an execution plan.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExpressionCodecError> {
        if bytes.len() > EXPRESSION_MAX_BYTES {
            return Err(ExpressionCodecError::LimitExceeded);
        }

        let mut reader = Reader {
            bytes,
            position: 0,
        };
        if reader.take(4)? != EXPRESSION_FORMAT_MAGIC {
            return Err(ExpressionCodecError::InvalidHeader);
        }
        if reader.u32()? != EXPRESSION_FORMAT_VERSION {
            return Err(ExpressionCodecError::UnsupportedVersion);
        }
        let input_count = reader.count()?;
        let node_count = reader.count()?;
        let output = reader.index()?;
        if output >= node_count {
            return Err(ExpressionPrepareError::InvalidReference.into());
        }

        // Each input needs at least 5 bytes, each node at least 5.
        // Counts are bounded before arithmetic/reservations on every target.
        if input_count * 5 + node_count * 5 > reader.remaining() {
            return Err(ExpressionCodecError::Truncated);
        }

        let mut inputs = Vec::new();
        inputs
            .try_reserve_exact(input_count)
            .map_err(|_| ExpressionCodecError::AllocationFailed)?;
        for _ in 0..input_count {
            let name = reader.text()?;
            let kind = decode_kind(reader.u8()?)?;
            let mut owned = String::new();
            owned
                .try_reserve_exact(name.len())
                .map_err(|_| ExpressionCodecError::AllocationFailed)?;
            owned.push_str(name);
            inputs.push(ExpressionInput {
                name: owned,
                kind,
            });
        }

        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(node_count)
            .map_err(|_| ExpressionCodecError::AllocationFailed)?;
        for _ in 0..node_count {
            let node = reader.node()?;
            if let ExpressionNode::Input(slot) = &node
                && *slot >= input_count
            {
                return Err(ExpressionPrepareError::InvalidReference.into());
            }
            if node
                .children()
                .iter()
                .flatten()
                .any(|&child| child >= node_count)
            {
                return Err(ExpressionPrepareError::InvalidReference.into());
            }
            nodes.push(node);
        }

        if reader.remaining() != 0 {
            return Err(ExpressionCodecError::TrailingBytes);
        }

        let declaration = Self {
            inputs,
            nodes,
            output,
        };
        validate_declaration(&declaration)?;
        Ok(declaration)
    }
}

#[derive(Default)]
struct Writer<'a> {
    bytes: Option<&'a mut Vec<u8>>,
    length: usize,
}

impl Writer<'_> {
    fn put(&mut self, value: &[u8]) -> Result<(), ExpressionCodecError> {
        self.length = self
            .length
            .checked_add(value.len())
            .filter(|&n| n <= EXPRESSION_MAX_BYTES)
            .ok_or(ExpressionCodecError::LimitExceeded)?;
        if let Some(bytes) = &mut self.bytes {
            bytes.extend_from_slice(value);
        }
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), ExpressionCodecError> {
        self.put(&[value])
    }

    fn index(&mut self, value: usize) -> Result<(), ExpressionCodecError> {
        self.put(
            &u32::try_from(value)
                .map_err(|_| ExpressionCodecError::LimitExceeded)?
                .to_le_bytes(),
        )
    }

    fn text(&mut self, text: &str) -> Result<(), ExpressionCodecError> {
        if text.len() > EXPRESSION_MAX_STRING_BYTES {
            return Err(ExpressionCodecError::LimitExceeded);
        }

        self.index(text.len())?;
        self.put(text.as_bytes())
    }

    fn value(&mut self, value: &DynamicValue) -> Result<(), ExpressionCodecError> {
        self.u8(encode_kind(value.kind())?)?;
        if let Some(lanes) = value.floats() {
            for lane in lanes {
                self.put(&lane.to_le_bytes())?;
            }
            return Ok(());
        }

        match value {
            DynamicValue::I32(value) => self.put(&value.to_le_bytes()),
            DynamicValue::U32(value) => self.put(&value.to_le_bytes()),
            DynamicValue::Bool(value) => self.put(&u32::from(*value).to_le_bytes()),
            DynamicValue::Text(value) => self.text(value),
            _ => Err(ExpressionCodecError::InvalidTag),
        }
    }
}

fn encode_declaration(
    writer: &mut Writer<'_>,
    declaration: &ExpressionDeclaration,
) -> Result<(), ExpressionCodecError> {
    if declaration.inputs.len() > EXPRESSION_MAX_ITEMS as usize
        || declaration.nodes.len() > EXPRESSION_MAX_ITEMS as usize
    {
        return Err(ExpressionCodecError::LimitExceeded);
    }

    writer.put(&EXPRESSION_FORMAT_MAGIC)?;
    writer.put(&EXPRESSION_FORMAT_VERSION.to_le_bytes())?;
    writer.index(declaration.inputs.len())?;
    writer.index(declaration.nodes.len())?;
    writer.index(declaration.output)?;

    for input in &declaration.inputs {
        writer.text(&input.name)?;
        writer.u8(encode_kind(input.kind)?)?;
    }

    for node in &declaration.nodes {
        match node {
            ExpressionNode::Input(slot) => {
                writer.u8(0)?;
                writer.index(*slot)?;
            }
            ExpressionNode::Constant(value) => {
                writer.u8(1)?;
                writer.value(value)?;
            }
            ExpressionNode::Unary {
                operator,
                operand,
            } => {
                writer.u8(2)?;
                writer.u8(encode_unary(*operator))?;
                writer.index(*operand)?;
            }
            ExpressionNode::Binary {
                operator,
                left,
                right,
            } => {
                writer.u8(3)?;
                writer.u8(encode_binary(*operator))?;
                writer.index(*left)?;
                writer.index(*right)?;
            }
            ExpressionNode::Clamp {
                value,
                minimum,
                maximum,
            } => {
                writer.u8(4)?;
                writer.index(*value)?;
                writer.index(*minimum)?;
                writer.index(*maximum)?;
            }
            ExpressionNode::Ternary {
                condition,
                then_node,
                else_node,
            } => {
                writer.u8(5)?;
                writer.index(*condition)?;
                writer.index(*then_node)?;
                writer.index(*else_node)?;
            }
            ExpressionNode::Fallback {
                value,
                replacement,
            } => {
                writer.u8(6)?;
                writer.index(*value)?;
                writer.index(*replacement)?;
            }
        }
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ExpressionCodecError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(ExpressionCodecError::LimitExceeded)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(ExpressionCodecError::Truncated)?;
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ExpressionCodecError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ExpressionCodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn index(&mut self) -> Result<usize, ExpressionCodecError> {
        usize::try_from(self.u32()?).map_err(|_| ExpressionCodecError::LimitExceeded)
    }

    fn count(&mut self) -> Result<usize, ExpressionCodecError> {
        let count = self.u32()?;
        if count > EXPRESSION_MAX_ITEMS {
            return Err(ExpressionCodecError::LimitExceeded);
        }
        Ok(count as usize)
    }

    fn text(&mut self) -> Result<&'a str, ExpressionCodecError> {
        let count = self.index()?;
        if count > EXPRESSION_MAX_STRING_BYTES {
            return Err(ExpressionCodecError::LimitExceeded);
        }

        std::str::from_utf8(self.take(count)?).map_err(|_| ExpressionCodecError::InvalidUtf8)
    }

    fn value(&mut self) -> Result<DynamicValue, ExpressionCodecError> {
        let start = self.position;
        let kind = decode_kind(self.u8()?)?;
        if kind == DynamicPropertyKind::Text {
            // Arc<str> uses a bounded standard-library allocation after length/UTF-8 checks.
            return Ok(DynamicValue::Text(Arc::from(self.text()?)));
        }
        self.take(kind.byte_len())?;
        DynamicValue::decode(&self.bytes[start..self.position])
            .map_err(|_| ExpressionCodecError::InvalidValue)
    }

    fn node(&mut self) -> Result<ExpressionNode, ExpressionCodecError> {
        Ok(match self.u8()? {
            0 => ExpressionNode::Input(self.index()?),
            1 => ExpressionNode::Constant(self.value()?),
            2 => ExpressionNode::Unary {
                operator: decode_unary(self.u8()?)?,
                operand: self.index()?,
            },
            3 => ExpressionNode::Binary {
                operator: decode_binary(self.u8()?)?,
                left: self.index()?,
                right: self.index()?,
            },
            4 => ExpressionNode::Clamp {
                value: self.index()?,
                minimum: self.index()?,
                maximum: self.index()?,
            },
            5 => ExpressionNode::Ternary {
                condition: self.index()?,
                then_node: self.index()?,
                else_node: self.index()?,
            },
            6 => ExpressionNode::Fallback {
                value: self.index()?,
                replacement: self.index()?,
            },
            _ => return Err(ExpressionCodecError::InvalidTag),
        })
    }
}

// Match explicitly: Rust enum declaration order/layout never assigns wire identity.
fn encode_kind(kind: DynamicPropertyKind) -> Result<u8, ExpressionCodecError> {
    Ok(match kind {
        DynamicPropertyKind::F32 => 1,
        DynamicPropertyKind::I32 => 2,
        DynamicPropertyKind::U32 => 3,
        DynamicPropertyKind::Bool => 4,
        DynamicPropertyKind::Vec2 => 5,
        DynamicPropertyKind::Vec3 => 6,
        DynamicPropertyKind::Vec4 => 7,
        DynamicPropertyKind::Mat2 => 8,
        DynamicPropertyKind::Mat3 => 9,
        DynamicPropertyKind::Mat4 => 10,
        DynamicPropertyKind::Text => 13,
        DynamicPropertyKind::Asset => return Err(ExpressionCodecError::InvalidTag),
    })
}

fn decode_kind(tag: u8) -> Result<DynamicPropertyKind, ExpressionCodecError> {
    Ok(match tag {
        1 => DynamicPropertyKind::F32,
        2 => DynamicPropertyKind::I32,
        3 => DynamicPropertyKind::U32,
        4 => DynamicPropertyKind::Bool,
        5 => DynamicPropertyKind::Vec2,
        6 => DynamicPropertyKind::Vec3,
        7 => DynamicPropertyKind::Vec4,
        8 => DynamicPropertyKind::Mat2,
        9 => DynamicPropertyKind::Mat3,
        10 => DynamicPropertyKind::Mat4,
        13 => DynamicPropertyKind::Text,
        _ => return Err(ExpressionCodecError::InvalidTag),
    })
}

fn encode_unary(operator: UnaryOperator) -> u8 {
    match operator {
        UnaryOperator::Negate => 0,
        UnaryOperator::Absolute => 1,
        UnaryOperator::Not => 2,
        UnaryOperator::Length => 3,
    }
}

fn decode_unary(tag: u8) -> Result<UnaryOperator, ExpressionCodecError> {
    Ok(match tag {
        0 => UnaryOperator::Negate,
        1 => UnaryOperator::Absolute,
        2 => UnaryOperator::Not,
        3 => UnaryOperator::Length,
        _ => return Err(ExpressionCodecError::InvalidTag),
    })
}

fn encode_binary(operator: BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::Add => 0,
        BinaryOperator::Subtract => 1,
        BinaryOperator::Multiply => 2,
        BinaryOperator::Divide => 3,
        BinaryOperator::Minimum => 4,
        BinaryOperator::Maximum => 5,
        BinaryOperator::Equal => 6,
        BinaryOperator::Less => 7,
        BinaryOperator::Greater => 8,
        BinaryOperator::And => 9,
        BinaryOperator::Or => 10,
    }
}

fn decode_binary(tag: u8) -> Result<BinaryOperator, ExpressionCodecError> {
    Ok(match tag {
        0 => BinaryOperator::Add,
        1 => BinaryOperator::Subtract,
        2 => BinaryOperator::Multiply,
        3 => BinaryOperator::Divide,
        4 => BinaryOperator::Minimum,
        5 => BinaryOperator::Maximum,
        6 => BinaryOperator::Equal,
        7 => BinaryOperator::Less,
        8 => BinaryOperator::Greater,
        9 => BinaryOperator::And,
        10 => BinaryOperator::Or,
        _ => return Err(ExpressionCodecError::InvalidTag),
    })
}

impl ExpressionDeclaration {
    /// Decode a bounded IPPE graph from windows into its private typed declaration.
    pub async fn decode_reader(
        reader: &mut dyn crate::services::io::IoReader,
    ) -> Result<Self, String> {
        use crate::services::asset_management::decode::{AssetReader, push};
        async fn count(input: &mut AssetReader<'_>) -> Result<usize, String> {
            let count = input.u32().await?;
            if count > EXPRESSION_MAX_ITEMS {
                return Err(ExpressionCodecError::LimitExceeded.to_string());
            }
            Ok(count as usize)
        }
        async fn text(input: &mut AssetReader<'_>) -> Result<String, String> {
            let length = input.u32().await? as usize;
            if length > EXPRESSION_MAX_STRING_BYTES
                || input
                    .position()
                    .checked_add(length as u64)
                    .is_none_or(|end| end > EXPRESSION_MAX_BYTES as u64)
            {
                return Err(ExpressionCodecError::LimitExceeded.to_string());
            }
            input.text(length).await
        }
        async fn value(input: &mut AssetReader<'_>) -> Result<DynamicValue, String> {
            let tag = input.u8().await?;
            let kind = decode_kind(tag).map_err(|error| error.to_string())?;
            if kind == DynamicPropertyKind::Text {
                return Ok(DynamicValue::Text(Arc::from(text(input).await?)));
            }
            let mut record = [0; 65];
            record[0] = tag;
            input.fill(&mut record[1..1 + kind.byte_len()]).await?;
            DynamicValue::decode(&record[..1 + kind.byte_len()])
                .map_err(|_| ExpressionCodecError::InvalidValue.to_string())
        }
        let mut input = AssetReader::new(reader);
        if input.array::<4>().await? != EXPRESSION_FORMAT_MAGIC {
            return Err(ExpressionCodecError::InvalidHeader.to_string());
        }
        if input.u32().await? != EXPRESSION_FORMAT_VERSION {
            return Err(ExpressionCodecError::UnsupportedVersion.to_string());
        }
        let input_count = count(&mut input).await?;
        let node_count = count(&mut input).await?;
        let output = input.u32().await? as usize;
        if output >= node_count {
            return Err("Invalid expression output reference".into());
        }
        let mut inputs = Vec::new();
        for _ in 0..input_count {
            let name = text(&mut input).await?;
            let kind = decode_kind(input.u8().await?).map_err(|error| error.to_string())?;
            push(
                &mut inputs,
                ExpressionInput {
                    name,
                    kind,
                },
            )?;
        }
        let mut nodes = Vec::new();
        for _ in 0..node_count {
            let node = match input.u8().await? {
                0 => ExpressionNode::Input(input.u32().await? as usize),
                1 => ExpressionNode::Constant(value(&mut input).await?),
                2 => ExpressionNode::Unary {
                    operator: decode_unary(input.u8().await?).map_err(|error| error.to_string())?,
                    operand: input.u32().await? as usize,
                },
                3 => ExpressionNode::Binary {
                    operator: decode_binary(input.u8().await?)
                        .map_err(|error| error.to_string())?,
                    left: input.u32().await? as usize,
                    right: input.u32().await? as usize,
                },
                4 => ExpressionNode::Clamp {
                    value: input.u32().await? as usize,
                    minimum: input.u32().await? as usize,
                    maximum: input.u32().await? as usize,
                },
                5 => ExpressionNode::Ternary {
                    condition: input.u32().await? as usize,
                    then_node: input.u32().await? as usize,
                    else_node: input.u32().await? as usize,
                },
                6 => ExpressionNode::Fallback {
                    value: input.u32().await? as usize,
                    replacement: input.u32().await? as usize,
                },
                _ => return Err(ExpressionCodecError::InvalidTag.to_string()),
            };
            if matches!(&node, ExpressionNode::Input(slot) if *slot >= input_count)
                || node
                    .children()
                    .iter()
                    .flatten()
                    .any(|&child| child >= node_count)
            {
                return Err("Invalid expression reference".into());
            }
            push(&mut nodes, node)?;
            if input.position() > EXPRESSION_MAX_BYTES as u64 {
                return Err(ExpressionCodecError::LimitExceeded.to_string());
            }
        }
        input.finish().await?;
        let declaration = Self {
            inputs,
            nodes,
            output,
        };
        super::preparation::validate_declaration_async(&declaration)
            .await
            .map_err(|error| format!("Invalid expression declaration: {error:?}"))?;
        Ok(declaration)
    }
}

impl ExpressionDeclaration {
    pub(crate) async fn encode_output(
        &self,
        output: &mut crate::services::asset_management::export::AssetOutput,
    ) -> Result<(), String> {
        output.write(&EXPRESSION_FORMAT_MAGIC).await?;
        output.u32(EXPRESSION_FORMAT_VERSION).await?;
        output.count(self.inputs.len()).await?;
        output.count(self.nodes.len()).await?;
        output.count(self.output).await?;
        for input in &self.inputs {
            output.text(&input.name).await?;
            output
                .byte(encode_kind(input.kind).map_err(|error| error.to_string())?)
                .await?;
        }
        for node in &self.nodes {
            match node {
                ExpressionNode::Input(slot) => {
                    output.byte(0).await?;
                    output.count(*slot).await?;
                }
                ExpressionNode::Constant(value) => {
                    output.byte(1).await?;
                    if let DynamicValue::Text(text) = value {
                        output
                            .byte(encode_kind(value.kind()).map_err(|error| error.to_string())?)
                            .await?;
                        output.text(text).await?;
                    } else {
                        let mut bytes = Vec::with_capacity(65);
                        Writer {
                            bytes: Some(&mut bytes),
                            length: 0,
                        }
                        .value(value)
                        .map_err(|error| error.to_string())?;
                        output.write(&bytes).await?;
                    }
                }
                ExpressionNode::Unary {
                    operator,
                    operand,
                } => {
                    output.write(&[2, encode_unary(*operator)]).await?;
                    output.count(*operand).await?;
                }
                ExpressionNode::Binary {
                    operator,
                    left,
                    right,
                } => {
                    output.write(&[3, encode_binary(*operator)]).await?;
                    output.count(*left).await?;
                    output.count(*right).await?;
                }
                ExpressionNode::Clamp {
                    value,
                    minimum,
                    maximum,
                } => {
                    output.byte(4).await?;
                    for index in [value, minimum, maximum] {
                        output.count(*index).await?;
                    }
                }
                ExpressionNode::Ternary {
                    condition,
                    then_node,
                    else_node,
                } => {
                    output.byte(5).await?;
                    for index in [condition, then_node, else_node] {
                        output.count(*index).await?;
                    }
                }
                ExpressionNode::Fallback {
                    value,
                    replacement,
                } => {
                    output.byte(6).await?;
                    output.count(*value).await?;
                    output.count(*replacement).await?;
                }
            }
        }
        Ok(())
    }
}
