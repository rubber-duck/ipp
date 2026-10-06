use super::*;
use crate::codec::{ProtocolError, Reader, Writer};
use crate::contract::wire_manifest::*;
use ipp_core::components::{DynamicPropertyKind, DynamicValue};
use ipp_core::services::data::{DataColumn, DataDelta, DataRowId, DataSchema, DataSourceKind};

pub(super) fn text(reader: &mut Reader<'_>, max: usize) -> Result<String, ProtocolError> {
    let length = reader.count(max)?;
    std::str::from_utf8(reader.take(length)?)
        .map(String::from)
        .map_err(|_| ProtocolError::Malformed("dataset UTF-8"))
}

pub(super) fn kind(reader: &mut Reader<'_>) -> Result<DataSourceKind, ProtocolError> {
    match reader.u8()? {
        DATASET_KIND_BUFFER => Ok(DataSourceKind::Buffer),
        DATASET_KIND_STREAMING => Ok(DataSourceKind::Streaming),
        tag => Err(ProtocolError::Unsupported(tag)),
    }
}

pub(super) fn write_kind(writer: &mut Writer, kind: DataSourceKind) -> Result<(), ProtocolError> {
    writer.u8(match kind {
        DataSourceKind::Buffer => DATASET_KIND_BUFFER,
        DataSourceKind::Streaming => DATASET_KIND_STREAMING,
    })
}

pub(super) fn schema(reader: &mut Reader<'_>) -> Result<DataSchema, ProtocolError> {
    let count = reader.count(COLUMNS)?;
    let mut columns = Vec::new();
    columns
        .try_reserve_exact(count)
        .map_err(|_| ProtocolError::Limit("dataset allocation"))?;
    for _ in 0..count {
        let name = text(reader, NAME_BYTES)?;
        let tag = reader.u8()?;
        let kind = if tag == 13 {
            DynamicPropertyKind::Text
        } else {
            DynamicPropertyKind::from_tag(tag).map_err(|_| ProtocolError::Unsupported(tag))?
        };
        let text_max_bytes = if reader.boolean()? {
            Some(
                usize::try_from(reader.u64()?)
                    .map_err(|_| ProtocolError::Limit("dataset text bound"))?,
            )
        } else {
            None
        };
        columns.push(DataColumn {
            name,
            kind,
            text_max_bytes,
        });
    }
    // Domain validation belongs to DataService; duplicate names and Asset columns are refusals there.
    Ok(DataSchema {
        columns,
    })
}

pub(super) fn write_schema(writer: &mut Writer, schema: &DataSchema) -> Result<(), ProtocolError> {
    writer.count(schema.columns.len(), COLUMNS)?;
    for column in &schema.columns {
        if column.name.len() > NAME_BYTES {
            return Err(ProtocolError::Limit("dataset column name"));
        }
        writer.string(&column.name)?;
        writer.u8(column.kind as u8)?;
        writer.u8(u8::from(column.text_max_bytes.is_some()))?;
        if let Some(bound) = column.text_max_bytes {
            writer.u64(bound as u64)?;
        }
    }
    Ok(())
}

fn floats<const N: usize>(reader: &mut Reader<'_>) -> Result<[f32; N], ProtocolError> {
    let mut values = [0.0; N];
    for value in &mut values {
        *value = f32::from_le_bytes(reader.take(4)?.try_into().unwrap());
    }
    Ok(values)
}

fn value(reader: &mut Reader<'_>) -> Result<DynamicValue, ProtocolError> {
    // Representation decoding deliberately preserves nonfinite values. Only DataService
    // validates a delta's domain, so a later bad value cannot erase a committed prefix.
    Ok(match reader.u8()? {
        DATASET_VALUE_F32 => DynamicValue::F32(floats::<1>(reader)?[0]),
        DATASET_VALUE_I32 => DynamicValue::I32(reader.u32()? as i32),
        DATASET_VALUE_U32 => DynamicValue::U32(reader.u32()?),
        DATASET_VALUE_BOOL => DynamicValue::Bool(reader.boolean()?),
        DATASET_VALUE_VEC2 => DynamicValue::Vec2(floats(reader)?),
        DATASET_VALUE_VEC3 => DynamicValue::Vec3(floats(reader)?),
        DATASET_VALUE_VEC4 => DynamicValue::Vec4(floats(reader)?),
        DATASET_VALUE_MAT2 => DynamicValue::Mat2(floats(reader)?),
        DATASET_VALUE_MAT3 => DynamicValue::Mat3(floats(reader)?),
        DATASET_VALUE_MAT4 => DynamicValue::Mat4(floats(reader)?),
        DATASET_VALUE_TEXT => DynamicValue::Text(text(reader, UPDATE_BYTES)?.into()),
        tag => return Err(ProtocolError::Unsupported(tag)),
    })
}

pub(super) fn write_values(
    writer: &mut Writer,
    values: &[DynamicValue],
) -> Result<(), ProtocolError> {
    writer.count(values.len(), COLUMNS)?;
    for value in values {
        writer.u8(value.kind() as u8)?;
        if let Some(floats) = value.floats() {
            for number in floats {
                writer.raw(&number.to_le_bytes())?;
            }
        } else {
            match value {
                DynamicValue::I32(number) => writer.u32(*number as u32)?,
                DynamicValue::U32(number) => writer.u32(*number)?,
                DynamicValue::Bool(boolean) => writer.u8(u8::from(*boolean))?,
                DynamicValue::Text(string) => writer.string(string)?,
                DynamicValue::Asset(_) => {
                    return Err(ProtocolError::Malformed("dataset asset value"));
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

fn values(reader: &mut Reader<'_>) -> Result<Vec<DynamicValue>, ProtocolError> {
    let count = reader.count(COLUMNS)?;
    if count > reader.bytes.len() - reader.at {
        return Err(ProtocolError::Malformed("dataset value count"));
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| ProtocolError::Limit("dataset allocation"))?;
    for _ in 0..count {
        values.push(value(reader)?);
    }
    Ok(values)
}

fn rows(reader: &mut Reader<'_>) -> Result<Vec<Vec<DynamicValue>>, ProtocolError> {
    let count = reader.count(UPDATE_BYTES / 4)?;
    if count > (reader.bytes.len() - reader.at) / 4 {
        return Err(ProtocolError::Malformed("dataset row count"));
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|_| ProtocolError::Limit("dataset allocation"))?;
    for _ in 0..count {
        rows.push(values(reader)?);
    }
    Ok(rows)
}

/// Decode complete transport representation, without prevalidating domain deltas.
/// Collection allocations are bounded by both declared limits and remaining input.
pub fn decode_update(bytes: &[u8]) -> Result<Vec<DataDelta>, ProtocolError> {
    if bytes.len() > UPDATE_BYTES {
        return Err(ProtocolError::Limit("dataset update"));
    }
    let mut reader = Reader {
        bytes,
        at: 0,
    };
    let count = reader.count(DELTAS)?;
    if count > reader.bytes.len() - reader.at {
        return Err(ProtocolError::Malformed("dataset delta count"));
    }
    let mut deltas = Vec::new();
    deltas
        .try_reserve_exact(count)
        .map_err(|_| ProtocolError::Limit("dataset allocation"))?;
    for _ in 0..count {
        deltas.push(match reader.u8()? {
            DATASET_DELTA_APPEND => DataDelta::Append {
                rows: rows(&mut reader)?,
            },
            DATASET_DELTA_INSERT => DataDelta::Insert {
                index: usize::try_from(reader.u64()?).unwrap_or(usize::MAX),
                rows: rows(&mut reader)?,
            },
            DATASET_DELTA_EDIT => DataDelta::Edit {
                row: DataRowId(reader.u64()?),
                values: values(&mut reader)?,
            },
            DATASET_DELTA_REMOVE => DataDelta::Remove {
                row: DataRowId(reader.u64()?),
            },
            tag => return Err(ProtocolError::Unsupported(tag)),
        });
    }
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("dataset update trailing bytes"));
    }
    Ok(deltas)
}

/// Encode a finite ordered logical update; integers keep their exact core width.
pub fn encode_update(deltas: &[DataDelta]) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::new());
    writer.count(deltas.len(), DELTAS)?;
    for delta in deltas {
        let rows = match delta {
            DataDelta::Append {
                rows,
            } => {
                writer.u8(DATASET_DELTA_APPEND)?;
                Some(rows)
            }
            DataDelta::Insert {
                index,
                rows,
            } => {
                writer.u8(DATASET_DELTA_INSERT)?;
                writer.u64(*index as u64)?;
                Some(rows)
            }
            DataDelta::Edit {
                row,
                values,
            } => {
                writer.u8(DATASET_DELTA_EDIT)?;
                writer.u64(row.0)?;
                write_values(&mut writer, values)?;
                None
            }
            DataDelta::Remove {
                row,
            } => {
                writer.u8(DATASET_DELTA_REMOVE)?;
                writer.u64(row.0)?;
                None
            }
        };
        if let Some(rows) = rows {
            writer.count(rows.len(), UPDATE_BYTES / 4)?;
            for row in rows {
                write_values(&mut writer, row)?;
            }
        }
    }
    Ok(writer.0)
}
