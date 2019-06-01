use crate::err::{self, Result};
use crate::evtx_parser::ReadSeek;
use snafu::{OptionExt, ResultExt};

pub use byteorder::{LittleEndian, ReadBytesExt};
use winstructs::guid::Guid;

use crate::model::deserialized::*;
use std::io::Cursor;

use crate::binxml::deserializer::BinXmlDeserializer;
use crate::binxml::name::BinXmlName;
use crate::binxml::value_variant::{BinXmlValue, BinXmlValueType};

use log::{trace, warn};

use std::io::Seek;
use std::io::SeekFrom;

use crate::evtx_chunk::EvtxChunk;
use encoding::EncodingRef;
use std::borrow::Cow;

pub fn read_template<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    chunk: Option<&'a EvtxChunk<'a>>,
    ansi_codec: EncodingRef,
) -> Result<BinXmlTemplate<'a>> {
    trace!("TemplateInstance at {}", cursor.position());

    let _ = try_read!(cursor, u8);
    let _template_id = try_read!(cursor, u32);
    let template_definition_data_offset = try_read!(cursor, u32);

    // If name is cached, read it and seek ahead if needed.
    let template_def = if let Some(definition) = chunk.and_then(|chunk| {
        chunk
            .template_table
            .get_template(template_definition_data_offset)
    }) {
        // Seek if needed
        trace!(
            "{} Got cached template from offset {}",
            cursor.position(),
            template_definition_data_offset
        );
        // 33 is template definition data size, we've read 9 bytes so far.
        if template_definition_data_offset == cursor.position() as u32 {
            cursor.seek(SeekFrom::Current(
                i64::from(definition.data_size) + (33 - 9),
            ))?;
        }
        Cow::Borrowed(definition)
    } else if template_definition_data_offset != cursor.position() as u32 {
        trace!(
            "Need to seek to offset {} to read template",
            template_definition_data_offset
        );
        let position_before_seek = cursor.position();

        cursor.seek(SeekFrom::Start(u64::from(template_definition_data_offset)))?;

        let template_def = read_template_definition(cursor, chunk, ansi_codec)?;

        cursor.seek(SeekFrom::Start(position_before_seek))?;

        Cow::Owned(template_def)
    } else {
        Cow::Owned(read_template_definition(cursor, chunk, ansi_codec)?)
    };

    let number_of_substitutions = try_read!(cursor, u32);

    let mut value_descriptors = Vec::with_capacity(number_of_substitutions as usize);

    for _ in 0..number_of_substitutions {
        let size = try_read!(cursor, u16);
        let value_type_token = try_read!(cursor, u8);

        let value_type =
            BinXmlValueType::from_u8(value_type_token).context(err::InvalidValueVariant {
                value: value_type_token,
                offset: cursor.position(),
            })?;

        // Empty
        let _ = try_read!(cursor, u8);

        value_descriptors.push(TemplateValueDescriptor { size, value_type })
    }

    trace!("{:?}", value_descriptors);

    let mut substitution_array = Vec::with_capacity(number_of_substitutions as usize);

    for descriptor in value_descriptors {
        let position_before_reading_value = cursor.position();
        trace!(
            "Substitution: {:?} at {}",
            descriptor.value_type,
            position_before_reading_value
        );
        let value = BinXmlValue::deserialize_value_type(
            &descriptor.value_type,
            cursor,
            chunk,
            Some(descriptor.size),
            ansi_codec,
        )?;

        trace!("\t {:?}", value);
        // NullType can mean deleted substitution (and data need to be skipped)
        if value == BinXmlValue::NullType {
            trace!("\t Skip {}", descriptor.size);
            cursor.seek(SeekFrom::Current(i64::from(descriptor.size)))?;
        }

        let current_position = cursor.position();
        let expected_position = position_before_reading_value + u64::from(descriptor.size);

        if expected_position != current_position {
            let diff = expected_position - current_position;
            // This sometimes occurs with dirty samples, but it's usually still possible to recover the rest of the record.
            // Sometimes however the log will contain a lot of zero fields.
            warn!("Read incorrect amount of data, cursor position is at {}, but should have ended up at {}, last descriptor was {:?}.",
                  current_position,
                  expected_position,
                  &descriptor);

            cursor.seek(SeekFrom::Start((current_position + diff) as u64))?;
        }
        substitution_array.push(value);
    }

    Ok(BinXmlTemplate {
        definition: template_def,
        substitution_array,
    })
}

pub fn read_template_definition<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    chunk: Option<&'a EvtxChunk<'a>>,
    ansi_codec: EncodingRef,
) -> Result<BinXMLTemplateDefinition<'a>> {
    let next_template_offset = try_read!(cursor, u32);

    let template_guid = try_read!(cursor, guid);
    // Data size includes the fragment header, element and end of file token;
    // except for the first 33 bytes of the template definition (above)
    let data_size = try_read!(cursor, u32);
    let tokens = BinXmlDeserializer::read_binxml_fragment(
        cursor,
        chunk,
        Some(data_size),
        false,
        ansi_codec,
    )?;

    Ok(BinXMLTemplateDefinition {
        next_template_offset,
        template_guid,
        data_size,
        tokens,
    })
}

pub fn read_entity_ref<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    chunk: Option<&'a EvtxChunk<'a>>,
) -> Result<BinXmlEntityReference<'a>> {
    trace!("EntityReference at {}", cursor.position());
    let name = BinXmlName::from_binxml_stream(cursor, chunk)?;
    trace!("\t name: {:?}", name);

    Ok(BinXmlEntityReference { name })
}

pub fn read_attribute<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    chunk: Option<&'a EvtxChunk<'a>>,
) -> Result<BinXMLAttribute<'a>> {
    let name = BinXmlName::from_binxml_stream(cursor, chunk)?;

    Ok(BinXMLAttribute { name })
}

pub fn read_fragment_header(cursor: &mut Cursor<&[u8]>) -> Result<BinXMLFragmentHeader> {
    let major_version = try_read!(cursor, u8);
    let minor_version = try_read!(cursor, u8);
    let flags = try_read!(cursor, u8);
    Ok(BinXMLFragmentHeader {
        major_version,
        minor_version,
        flags,
    })
}

pub fn read_substitution_descriptor(
    cursor: &mut Cursor<&[u8]>,
    optional: bool,
) -> Result<TemplateSubstitutionDescriptor> {
    let substitution_index = try_read!(cursor, u16);
    let value_type_token = try_read!(cursor, u8);

    let value_type =
        BinXmlValueType::from_u8(value_type_token).context(err::InvalidValueVariant {
            value: value_type_token,
            offset: cursor.position(),
        })?;

    let ignore = optional && (value_type == BinXmlValueType::NullType);

    Ok(TemplateSubstitutionDescriptor {
        substitution_index,
        value_type,
        ignore,
    })
}

pub fn read_open_start_element<'a>(
    cursor: &mut Cursor<&'a [u8]>,
    chunk: Option<&'a EvtxChunk<'a>>,
    has_attributes: bool,
    is_substitution: bool,
) -> Result<BinXMLOpenStartElement<'a>> {
    if !is_substitution {
        // Reserved
        let _ = try_read!(cursor, u16);
    }
    let data_size = try_read!(cursor, u32);
    let name = BinXmlName::from_binxml_stream(cursor, chunk)?;

    let _attribute_list_data_size = if has_attributes {
        try_read!(cursor, u32)
    } else {
        0
    };

    Ok(BinXMLOpenStartElement { data_size, name })
}

#[cfg(test)]
mod test {
    use crate::binxml::name::BinXmlName;
    use crate::binxml::value_variant::BinXmlValueType::*;
    use crate::model::deserialized::BinXMLDeserializedTokens::*;
    use crate::model::deserialized::*;
    use pretty_assertions::assert_eq;

    use crate::binxml::tokens::read_template_definition;
    use crate::binxml::value_variant::BinXmlValue;
    use crate::ensure_env_logger_initialized;
    use encoding::all::WINDOWS_1252;
    use std::borrow::Cow;
    use std::io::{Cursor, Seek, SeekFrom};
    use winstructs::guid::Guid;

    macro_rules! n {
        ($s: expr) => {
            BinXmlName::from_static_string($s)
        };
    }

    #[test]
    fn test_read_template_definition() {
        ensure_env_logger_initialized();
        let expected_at_550 = BinXMLTemplateDefinition {
            next_template_offset: 0,
            template_guid: Guid::new(
                3_346_188_909,
                47309,
                26506,
                [241, 69, 105, 59, 93, 11, 147, 140],
            ),
            data_size: 1170,
            tokens: vec![
                FragmentHeader(BinXMLFragmentHeader {
                    major_version: 1,
                    minor_version: 1,
                    flags: 0,
                }),
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 1158,
                    name: n!("Event"),
                }),
                Attribute(BinXMLAttribute { name: n!("xmlns") }),
                Value(Cow::Owned(BinXmlValue::StringType(Cow::Borrowed(
                    "http://schemas.microsoft.com/win/2004/08/events/event",
                )))),
                CloseStartElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 982,
                    name: n!("System"),
                }),
                CloseStartElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 89,
                    name: n!("Provider"),
                }),
                Attribute(BinXMLAttribute { name: n!("Name") }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 14,
                    value_type: StringType,
                    ignore: false,
                }),
                Attribute(BinXMLAttribute { name: n!("Guid") }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 15,
                    value_type: GuidType,
                    ignore: false,
                }),
                CloseEmptyElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 77,
                    name: n!("EventID"),
                }),
                Attribute(BinXMLAttribute {
                    name: n!("Qualifiers"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 4,
                    value_type: UInt16Type,
                    ignore: false,
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 3,
                    value_type: UInt16Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 34,
                    name: n!("Version"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 11,
                    value_type: UInt8Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 30,
                    name: n!("Level"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 0,
                    value_type: UInt8Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 28,
                    name: n!("Task"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 2,
                    value_type: UInt16Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 32,
                    name: n!("Opcode"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 1,
                    value_type: UInt8Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 36,
                    name: n!("Keywords"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 5,
                    value_type: HexInt64Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 80,
                    name: n!("TimeCreated"),
                }),
                Attribute(BinXMLAttribute {
                    name: n!("SystemTime"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 6,
                    value_type: FileTimeType,
                    ignore: false,
                }),
                CloseEmptyElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 46,
                    name: n!("EventRecordID"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 10,
                    value_type: UInt64Type,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 133,
                    name: n!("Correlation"),
                }),
                Attribute(BinXMLAttribute {
                    name: n!("ActivityID"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 7,
                    value_type: GuidType,
                    ignore: false,
                }),
                Attribute(BinXMLAttribute {
                    name: n!("RelatedActivityID"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 13,
                    value_type: GuidType,
                    ignore: false,
                }),
                CloseEmptyElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 109,
                    name: n!("Execution"),
                }),
                Attribute(BinXMLAttribute {
                    name: n!("ProcessID"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 8,
                    value_type: UInt32Type,
                    ignore: false,
                }),
                Attribute(BinXMLAttribute {
                    name: n!("ThreadID"),
                }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 9,
                    value_type: UInt32Type,
                    ignore: false,
                }),
                CloseEmptyElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 34,
                    name: n!("Channel"),
                }),
                CloseStartElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 16,
                    value_type: StringType,
                    ignore: false,
                }),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 62,
                    name: n!("Computer"),
                }),
                CloseStartElement,
                Value(Cow::Owned(BinXmlValue::StringType(Cow::Borrowed(
                    "37L4247F27-25",
                )))),
                CloseElement,
                OpenStartElement(BinXMLOpenStartElement {
                    data_size: 66,
                    name: n!("Security"),
                }),
                Attribute(BinXMLAttribute { name: n!("UserID") }),
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 12,
                    value_type: SidType,
                    ignore: false,
                }),
                CloseEmptyElement,
                CloseElement,
                Substitution(TemplateSubstitutionDescriptor {
                    substitution_index: 17,
                    value_type: BinXmlType,
                    ignore: false,
                }),
                CloseElement,
                EndOfStream,
            ],
        };
        let evtx_file = include_bytes!("../../samples/security.evtx");
        let from_start_of_chunk = &evtx_file[4096..];

        let mut c = Cursor::new(from_start_of_chunk);
        c.seek(SeekFrom::Start(550)).unwrap();
        let actual = read_template_definition(&mut c, None, WINDOWS_1252).unwrap();

        assert_eq!(actual, expected_at_550);
    }
}
