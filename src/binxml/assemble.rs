use crate::err::{EvtxError, Result};

use crate::binxml::value_variant::BinXmlValue;
use crate::model::deserialized::{
    BinXMLDeserializedTokens, BinXmlTemplateRef, TemplateSubstitutionDescriptor,
};
use crate::model::xml::{XmlElementBuilder, XmlModel, XmlPIBuilder};
use crate::xml_output::BinXmlOutput;
use log::{debug, trace, warn};
use std::borrow::Cow;

use std::mem;

use crate::binxml::tokens::read_template_definition;
use crate::EvtxChunk;
use std::io::{Cursor, Seek, SeekFrom};

pub fn parse_tokens<'a, T: BinXmlOutput>(
    tokens: Vec<BinXMLDeserializedTokens<'a>>,
    chunk: &'a EvtxChunk<'a>,
    visitor: &mut T,
) -> Result<()> {
    let expanded_tokens = expand_templates(tokens, chunk)?;
    let record_model = create_record_model(expanded_tokens)?;

    visitor.visit_start_of_stream()?;

    let mut stack = vec![];

    for owned_token in record_model {
        match owned_token {
            XmlModel::OpenElement(open_element) => {
                stack.push(open_element);
                visitor.visit_open_start_element(stack.last().ok_or(
                    EvtxError::FailedToCreateRecordModel(
                        "Invalid parser state - expected stack to be non-empty",
                    ),
                )?)?;
            }
            XmlModel::CloseElement => {
                let close_element = stack.pop().ok_or(EvtxError::FailedToCreateRecordModel(
                    "Invalid parser state - expected stack to be non-empty",
                ))?;
                visitor.visit_close_element(&close_element)?
            }
            XmlModel::Value(s) => visitor.visit_characters(&s)?,
            XmlModel::EndOfStream => {}
            XmlModel::StartOfStream => {}
            XmlModel::PI(pi) => visitor.visit_processing_instruction(&pi)?,
            XmlModel::EntityRef(entity) => visitor.visit_entity_reference(&entity)?,
        };
    }

    visitor.visit_end_of_stream()?;

    Ok(())
}

pub fn create_record_model<'a>(
    tokens: Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) -> Result<Vec<XmlModel<'a>>> {
    let mut current_element: Option<XmlElementBuilder> = None;
    let mut current_pi: Option<XmlPIBuilder> = None;
    let mut model: Vec<XmlModel> = Vec::with_capacity(tokens.len());

    for token in tokens {
        // Handle all places where we don't care if it's an Owned or a Borrowed value.
        match token {
            Cow::Owned(BinXMLDeserializedTokens::FragmentHeader(_))
            | Cow::Borrowed(BinXMLDeserializedTokens::FragmentHeader(_)) => {}
            Cow::Owned(BinXMLDeserializedTokens::TemplateInstance(_))
            | Cow::Borrowed(BinXMLDeserializedTokens::TemplateInstance(_)) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Call `expand_templates` before calling this function",
                ));
            }
            Cow::Owned(BinXMLDeserializedTokens::AttributeList)
            | Cow::Borrowed(BinXMLDeserializedTokens::AttributeList) => {}

            Cow::Owned(BinXMLDeserializedTokens::CloseElement)
            | Cow::Borrowed(BinXMLDeserializedTokens::CloseElement) => {
                model.push(XmlModel::CloseElement);
            }

            Cow::Owned(BinXMLDeserializedTokens::CloseStartElement)
            | Cow::Borrowed(BinXMLDeserializedTokens::CloseStartElement) => {
                trace!("BinXMLDeserializedTokens::CloseStartElement");
                match current_element.take() {
                    None => {
                        return Err(EvtxError::FailedToCreateRecordModel(
                            "close start - Bad parser state",
                        ));
                    }
                    Some(builder) => model.push(XmlModel::OpenElement(builder.finish()?)),
                };
            }
            Cow::Owned(BinXMLDeserializedTokens::CDATASection)
            | Cow::Borrowed(BinXMLDeserializedTokens::CDATASection) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Unimplemented - CDATA",
                ));
            }
            Cow::Owned(BinXMLDeserializedTokens::CharRef)
            | Cow::Borrowed(BinXMLDeserializedTokens::CharRef) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Unimplemented - CharacterReference",
                ));
            }
            Cow::Owned(BinXMLDeserializedTokens::EntityRef(entity)) => {
                model.push(XmlModel::EntityRef(Cow::Owned(entity.name)))
            }
            Cow::Borrowed(BinXMLDeserializedTokens::EntityRef(entity)) => {
                model.push(XmlModel::EntityRef(Cow::Borrowed(&entity.name)))
            }
            Cow::Owned(BinXMLDeserializedTokens::PITarget(name)) => {
                let builder = XmlPIBuilder::new();
                if let Some(_pi) = current_pi {
                    warn!("PITarget without following PIData, previous target will be ignored.")
                }
                current_pi = Some(builder.name(Cow::Owned(name.name)));
            }
            Cow::Borrowed(BinXMLDeserializedTokens::PITarget(name)) => {
                let builder = XmlPIBuilder::new();
                current_pi = Some(builder.name(Cow::Borrowed(&name.name)));
            }
            Cow::Owned(BinXMLDeserializedTokens::PIData(data)) => match current_pi.take() {
                None => {
                    return Err(EvtxError::FailedToCreateRecordModel(
                        "PI Data without PI target - Bad parser state",
                    ));
                }
                Some(builder) => {
                    model.push(builder.data(data).finish());
                }
            },
            Cow::Borrowed(BinXMLDeserializedTokens::PIData(data)) => match current_pi.take() {
                None => {
                    return Err(EvtxError::FailedToCreateRecordModel(
                        "PI Data without PI target - Bad parser state",
                    ));
                }
                Some(builder) => {
                    model.push(builder.data(Cow::Borrowed(data)).finish());
                }
            },
            Cow::Owned(BinXMLDeserializedTokens::Substitution(_))
            | Cow::Borrowed(BinXMLDeserializedTokens::Substitution(_)) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Call `expand_templates` before calling this function",
                ));
            }
            Cow::Owned(BinXMLDeserializedTokens::EndOfStream)
            | Cow::Borrowed(BinXMLDeserializedTokens::EndOfStream) => {
                model.push(XmlModel::EndOfStream)
            }
            Cow::Owned(BinXMLDeserializedTokens::StartOfStream)
            | Cow::Borrowed(BinXMLDeserializedTokens::StartOfStream) => {
                model.push(XmlModel::StartOfStream)
            }

            Cow::Owned(BinXMLDeserializedTokens::CloseEmptyElement)
            | Cow::Borrowed(BinXMLDeserializedTokens::CloseEmptyElement) => {
                trace!("BinXMLDeserializedTokens::CloseEmptyElement");
                match current_element.take() {
                    None => {
                        return Err(EvtxError::FailedToCreateRecordModel(
                            "close empty - Bad parser state",
                        ));
                    }
                    Some(builder) => {
                        model.push(XmlModel::OpenElement(builder.finish()?));
                        model.push(XmlModel::CloseElement);
                    }
                };
            }

            Cow::Owned(BinXMLDeserializedTokens::Attribute(attr)) => {
                trace!("BinXMLDeserializedTokens::Attribute(attr) - {:?}", attr);
                match current_element.take() {
                    None => {
                        return Err(EvtxError::FailedToCreateRecordModel(
                            "attribute - Bad parser state",
                        ));
                    }
                    Some(builder) => {
                        current_element = Some(builder.attribute_name(Cow::Owned(attr.name)));
                    }
                };
            }

            Cow::Borrowed(BinXMLDeserializedTokens::Attribute(attr)) => {
                trace!("BinXMLDeserializedTokens::Attribute(attr) - {:?}", attr);
                match current_element.take() {
                    None => {
                        return Err(EvtxError::FailedToCreateRecordModel(
                            "attribute - Bad parser state",
                        ));
                    }
                    Some(builder) => {
                        current_element = Some(builder.attribute_name(Cow::Borrowed(&attr.name)));
                    }
                };
            }

            Cow::Owned(BinXMLDeserializedTokens::OpenStartElement(elem)) => {
                trace!(
                    "BinXMLDeserializedTokens::OpenStartElement(elem) - {:?}",
                    elem.name
                );
                let builder = XmlElementBuilder::new();
                current_element = Some(builder.name(Cow::Owned(elem.name)));
            }
            Cow::Borrowed(BinXMLDeserializedTokens::OpenStartElement(elem)) => {
                trace!(
                    "BinXMLDeserializedTokens::OpenStartElement(elem) - {:?}",
                    elem.name
                );
                let builder = XmlElementBuilder::new();
                current_element = Some(builder.name(Cow::Borrowed(&elem.name)));
            }

            Cow::Owned(BinXMLDeserializedTokens::Value(value)) => {
                trace!("BinXMLDeserializedTokens::Value(value) - {:?}", value);
                match current_element.take() {
                    // A string that is not inside any element, yield it
                    None => match value {
                        BinXmlValue::EvtXml => {
                            return Err(EvtxError::FailedToCreateRecordModel(
                                "Call `expand_templates` before calling this function",
                            ));
                        }
                        _ => {
                            model.push(XmlModel::Value(Cow::Owned(value)));
                        }
                    },
                    // A string that is bound to an attribute
                    Some(builder) => {
                        current_element = Some(builder.attribute_value(Cow::Owned(value))?);
                    }
                };
            }
            Cow::Borrowed(BinXMLDeserializedTokens::Value(value)) => {
                trace!("BinXMLDeserializedTokens::Value(value) - {:?}", value);
                match current_element.take() {
                    // A string that is not inside any element, yield it
                    None => match value {
                        BinXmlValue::EvtXml => {
                            return Err(EvtxError::FailedToCreateRecordModel(
                                "Call `expand_templates` before calling this function",
                            ));
                        }
                        _ => {
                            model.push(XmlModel::Value(Cow::Borrowed(value)));
                        }
                    },
                    // A string that is bound to an attribute
                    Some(builder) => {
                        current_element = Some(builder.attribute_value(Cow::Borrowed(value))?);
                    }
                };
            }
        }
    }

    Ok(model)
}

fn expand_token_substitution<'a>(
    template: &mut BinXmlTemplateRef<'a>,
    substitution_descriptor: &TemplateSubstitutionDescriptor,
    chunk: &'a EvtxChunk<'a>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) -> Result<()> {
    if substitution_descriptor.ignore {
        return Ok(());
    }

    let value = template
        .substitution_array
        .get_mut(substitution_descriptor.substitution_index as usize);

    if let Some(value) = value {
        let value = mem::replace(
            value,
            BinXMLDeserializedTokens::Value(BinXmlValue::NullType),
        );
        _expand_templates(Cow::Owned(value), chunk, stack)?;
    } else {
        _expand_templates(
            Cow::Owned(BinXMLDeserializedTokens::Value(BinXmlValue::NullType)),
            chunk,
            stack,
        )?;
    }

    Ok(())
}

fn expand_template<'a>(
    mut template: BinXmlTemplateRef<'a>,
    chunk: &'a EvtxChunk<'a>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) -> Result<()> {
    if let Some(template_def) = chunk
        .template_table
        .get_template(template.template_def_offset)
    {
        // We expect to find all the templates in the template cache.
        for token in template_def.tokens.iter() {
            if let BinXMLDeserializedTokens::Substitution(ref substitution_descriptor) = token {
                expand_token_substitution(&mut template, substitution_descriptor, chunk, stack)?;
            } else {
                _expand_templates(Cow::Borrowed(token), chunk, stack)?;
            }
        }
    } else {
        // If the file was not closed correctly, there can be a template which was not found in the header.
        // In that case, we will try to read it directly from the chunk.
        debug!(
            "Template in offset {} was not found in cache",
            template.template_def_offset
        );

        let mut cursor = Cursor::new(chunk.data);

        let _ = cursor.seek(SeekFrom::Start(u64::from(template.template_def_offset)));
        let template_def =
            read_template_definition(&mut cursor, Some(chunk), chunk.settings.get_ansi_codec())?;

        for token in template_def.tokens {
            if let BinXMLDeserializedTokens::Substitution(ref substitution_descriptor) = token {
                expand_token_substitution(&mut template, substitution_descriptor, chunk, stack)?;
            } else {
                _expand_templates(Cow::Owned(token), chunk, stack)?;
            }
        }
    };

    Ok(())
}

fn _expand_templates<'a>(
    token: Cow<'a, BinXMLDeserializedTokens<'a>>,
    chunk: &'a EvtxChunk<'a>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) -> Result<()> {
    match token {
        // Owned values can be consumed when flatting, and passed on as owned.
        Cow::Owned(BinXMLDeserializedTokens::Value(BinXmlValue::BinXmlType(tokens))) => {
            for token in tokens.into_iter() {
                _expand_templates(Cow::Owned(token), chunk, stack)?;
            }
        }

        Cow::Borrowed(BinXMLDeserializedTokens::Value(BinXmlValue::BinXmlType(tokens))) => {
            for token in tokens.iter() {
                _expand_templates(Cow::Borrowed(token), chunk, stack)?;
            }
        }

        // Actual template handling.
        Cow::Owned(BinXMLDeserializedTokens::TemplateInstance(template)) => {
            expand_template(template, chunk, stack)?;
        }
        Cow::Borrowed(BinXMLDeserializedTokens::TemplateInstance(template)) => {
            // This can happen if a template has a token which is:
            // 1. Another template.
            // 2. Is not a substitution (because they are `Owned` values).
            // We never actually see this in practice, so we don't mind paying for `clone` here.
            expand_template(template.clone(), chunk, stack)?;
        }

        _ => stack.push(token),
    }

    Ok(())
}

pub fn expand_templates<'a>(
    token_tree: Vec<BinXMLDeserializedTokens<'a>>,
    chunk: &'a EvtxChunk<'a>,
) -> Result<Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>> {
    // We can assume the new tree will be at least as big as the old one.
    let mut stack = Vec::with_capacity(token_tree.len());

    for token in token_tree {
        _expand_templates(Cow::Owned(token), chunk, &mut stack)?
    }

    Ok(stack)
}
