use crate::binxml::name::BinXmlName;
use crate::binxml::value_variant::{BinXmlValue, BinXmlValueType};

use std::borrow::Cow;

use crate::ChunkOffset;
use winstructs::guid::Guid;

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub enum BinXMLDeserializedTokens<'a> {
    FragmentHeader(BinXMLFragmentHeader),
    TemplateInstance(BinXmlTemplateRef<'a>),
    OpenStartElement(BinXMLOpenStartElement<'a>),
    AttributeList,
    Attribute(BinXMLAttribute<'a>),
    CloseStartElement,
    CloseEmptyElement,
    CloseElement,
    Value(BinXmlValue<'a>),
    CDATASection,
    CharRef,
    EntityRef(BinXmlEntityReference<'a>),
    PITarget(BinXMLProcessingInstructionTarget<'a>),
    PIData(Cow<'a, str>),
    Substitution(TemplateSubstitutionDescriptor),
    EndOfStream,
    StartOfStream,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXMLProcessingInstructionTarget<'a> {
    pub name: BinXmlName<'a>,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXMLOpenStartElement<'a> {
    pub data_size: u32,
    pub name: BinXmlName<'a>,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXmlTemplateDefinitionHeader {
    /// A pointer to the next template in the bucket.
    pub next_template_offset: ChunkOffset,
    pub guid: Guid,
    pub data_size: u32,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXMLTemplateDefinition<'a> {
    pub header: BinXmlTemplateDefinitionHeader,
    pub tokens: Vec<BinXMLDeserializedTokens<'a>>,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXmlEntityReference<'a> {
    pub name: BinXmlName<'a>,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXmlTemplateRef<'a> {
    pub template_def_offset: ChunkOffset,
    pub substitution_array: Vec<BinXMLDeserializedTokens<'a>>,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct TemplateValueDescriptor {
    pub size: u16,
    pub value_type: BinXmlValueType,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct TemplateSubstitutionDescriptor {
    // Zero-based (0 is first replacement)
    pub substitution_index: u16,
    pub value_type: BinXmlValueType,
    pub ignore: bool,
}

#[repr(C)]
#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXMLFragmentHeader {
    pub major_version: u8,
    pub minor_version: u8,
    pub flags: u8,
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct BinXMLAttribute<'a> {
    pub name: BinXmlName<'a>,
}
