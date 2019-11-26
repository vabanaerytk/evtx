use crate::binxml::tokens::read_template_definition;
use crate::err::{DeserializationResult, WrappedIoError};

use crate::model::deserialized::BinXMLTemplateDefinition;
use crate::ChunkOffset;
pub use byteorder::{LittleEndian, ReadBytesExt};

use encoding::EncodingRef;
use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom};

pub type CachedTemplate<'chunk> = BinXMLTemplateDefinition<'chunk>;

#[derive(Debug, Default)]
pub struct TemplateCache<'chunk>(HashMap<ChunkOffset, CachedTemplate<'chunk>>);

impl<'chunk> TemplateCache<'chunk> {
    pub fn new() -> Self {
        TemplateCache(HashMap::new())
    }

    pub fn populate(
        data: &'chunk [u8],
        offsets: &[ChunkOffset],
        ansi_codec: EncodingRef,
    ) -> DeserializationResult<Self> {
        let mut cache = HashMap::new();
        let mut cursor = Cursor::new(data);

        for offset in offsets.iter().filter(|&&offset| offset > 0) {
            cursor
                .seek(SeekFrom::Start(u64::from(*offset)))
                .map_err(|e| {
                    WrappedIoError::io_error_with_message(
                        e,
                        format!("seeking to template at chunk offset {} failed.", offset),
                        &mut cursor,
                    )
                })?;

            let definition = read_template_definition(&mut cursor, None, ansi_codec)?;
            cache.insert(*offset, definition);
        }

        Ok(TemplateCache(cache))
    }

    pub fn get_template(&self, offset: ChunkOffset) -> Option<&CachedTemplate<'chunk>> {
        self.0.get(&offset)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}
