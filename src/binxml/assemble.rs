use crate::err::{EvtxError, Result};

use crate::binxml::value_variant::BinXmlValue;
use crate::model::deserialized::{BinXMLDeserializedTokens, BinXmlTemplate};
use crate::model::xml::{XmlElementBuilder, XmlModel, XmlPIBuilder};
use crate::xml_output::BinXmlOutput;
use log::{trace, warn};
use std::borrow::{Borrow, BorrowMut, Cow};

use std::mem;

pub fn parse_tokens<T: BinXmlOutput>(
    tokens: Vec<BinXMLDeserializedTokens>,
    visitor: &mut T,
) -> Result<()> {
    let expanded_tokens = expand_templates(tokens);
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
                        ))
                    }
                    Some(builder) => model.push(XmlModel::OpenElement(builder.finish()?)),
                };
            }
            Cow::Owned(BinXMLDeserializedTokens::CDATASection)
            | Cow::Borrowed(BinXMLDeserializedTokens::CDATASection) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Unimplemented - CDATA",
                ))
            }
            Cow::Owned(BinXMLDeserializedTokens::CharRef)
            | Cow::Borrowed(BinXMLDeserializedTokens::CharRef) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Unimplemented - CharacterReference",
                ))
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
                    ))
                }
                Some(builder) => {
                    model.push(builder.data(data).finish());
                }
            },
            Cow::Borrowed(BinXMLDeserializedTokens::PIData(data)) => match current_pi.take() {
                None => {
                    return Err(EvtxError::FailedToCreateRecordModel(
                        "PI Data without PI target - Bad parser state",
                    ))
                }
                Some(builder) => {
                    model.push(builder.data(Cow::Borrowed(data)).finish());
                }
            },
            Cow::Owned(BinXMLDeserializedTokens::Substitution(_))
            | Cow::Borrowed(BinXMLDeserializedTokens::Substitution(_)) => {
                return Err(EvtxError::FailedToCreateRecordModel(
                    "Call `expand_templates` before calling this function",
                ))
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
                        ))
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
                        ))
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
                        ))
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

            Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Owned(value))) => {
                trace!("BinXMLDeserializedTokens::Value(value) - {:?}", value);
                match current_element.take() {
                    // A string that is not inside any element, yield it
                    None => match value {
                        BinXmlValue::EvtXml => {
                            return Err(EvtxError::FailedToCreateRecordModel(
                                "Call `expand_templates` before calling this function",
                            ))
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
            Cow::Borrowed(BinXMLDeserializedTokens::Value(Cow::Owned(value)))
            | Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Borrowed(value))) => {
                trace!("BinXMLDeserializedTokens::Value(value) - {:?}", value);
                match current_element.take() {
                    // A string that is not inside any element, yield it
                    None => match value {
                        BinXmlValue::EvtXml => {
                            return Err(EvtxError::FailedToCreateRecordModel(
                                "Call `expand_templates` before calling this function",
                            ))
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

            // Same as above, but `value` is `&&BinXmlValue` which is not compatible with the match.
            Cow::Borrowed(BinXMLDeserializedTokens::Value(Cow::Borrowed(value))) => {
                trace!("BinXMLDeserializedTokens::Value(value) - {:?}", value);

                match current_element.take() {
                    // A string that is not inside any element, yield it
                    None => match value {
                        BinXmlValue::EvtXml => {
                            return Err(EvtxError::FailedToCreateRecordModel(
                                "Call `expand_templates` before calling this function",
                            ))
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

fn expand_owned_template<'a>(
    mut template: BinXmlTemplate<'a>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) {
    // If the template owns the definition, we can consume the tokens.
    let tokens: Vec<Cow<'a, BinXMLDeserializedTokens<'a>>> = match template.definition {
        Cow::Owned(owned_def) => owned_def.tokens.into_iter().map(Cow::Owned).collect(),
        Cow::Borrowed(ref_def) => ref_def.tokens.iter().map(Cow::Borrowed).collect(),
    };

    for token in tokens {
        if let BinXMLDeserializedTokens::Substitution(ref substitution_descriptor) = token.as_ref()
        {
            if substitution_descriptor.ignore {
                continue;
            } else {
                // We swap out the node in the substitution array with a dummy value (to avoid copying it),
                // moving control of the original node to the new token tree.
                let value = mem::replace(
                    template
                        .substitution_array
                        .get_mut(substitution_descriptor.substitution_index as usize)
                        .unwrap_or(BinXmlValue::NullType.borrow_mut()),
                    BinXmlValue::NullType,
                );

                _expand_templates(
                    Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Owned(value))),
                    stack,
                );
            }
        } else {
            _expand_templates(token, stack);
        }
    }
}

fn expand_borrowed_template<'a>(
    template: &'a BinXmlTemplate<'a>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) {
    // Here we can always use refs, since even if the definition is owned by the template,
    // we do not own it.
    for token in template.definition.as_ref().tokens.iter() {
        if let BinXMLDeserializedTokens::Substitution(ref substitution_descriptor) = token {
            if substitution_descriptor.ignore {
                continue;
            } else {
                let value = &template
                    .substitution_array
                    .get(substitution_descriptor.substitution_index as usize)
                    .unwrap_or(BinXmlValue::NullType.borrow());

                _expand_templates(
                    Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Borrowed(value))),
                    stack,
                );
            }
        } else {
            _expand_templates(Cow::Borrowed(token), stack);
        }
    }
}

fn _expand_templates<'a>(
    token: Cow<'a, BinXMLDeserializedTokens<'a>>,
    stack: &mut Vec<Cow<'a, BinXMLDeserializedTokens<'a>>>,
) {
    match token {
        // Owned values can be consumed when flatting, and passed on as owned.
        Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Owned(BinXmlValue::BinXmlType(
            tokens,
        )))) => {
            for token in tokens.into_iter() {
                _expand_templates(Cow::Owned(token), stack);
            }
        }

        // All borrowed values are flattened and kept borrowed.
        Cow::Owned(BinXMLDeserializedTokens::Value(Cow::Borrowed(BinXmlValue::BinXmlType(
            tokens,
        ))))
        | Cow::Borrowed(BinXMLDeserializedTokens::Value(Cow::Owned(BinXmlValue::BinXmlType(
            tokens,
        ))))
        | Cow::Borrowed(BinXMLDeserializedTokens::Value(Cow::Borrowed(BinXmlValue::BinXmlType(
            tokens,
        )))) => {
            for token in tokens.iter() {
                _expand_templates(Cow::Borrowed(token), stack);
            }
        }

        // Actual template handling.
        Cow::Owned(BinXMLDeserializedTokens::TemplateInstance(template)) => {
            expand_owned_template(template, stack);
        }
        Cow::Borrowed(BinXMLDeserializedTokens::TemplateInstance(template)) => {
            expand_borrowed_template(template, stack);
        }

        _ => stack.push(token),
    }
}

pub fn expand_templates(
    token_tree: Vec<BinXMLDeserializedTokens>,
) -> Vec<Cow<BinXMLDeserializedTokens>> {
    // We can assume the new tree will be at least as big as the old one.
    let mut stack = Vec::with_capacity(token_tree.len());

    for token in token_tree {
        _expand_templates(Cow::Owned(token), &mut stack)
    }

    stack
}
