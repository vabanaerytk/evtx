use std::fmt;
use std::fmt::{Debug, Display};

use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Result};

pub struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    pub fn new(data1: u32, data2: u16, data3: u16, data4: &[u8]) -> Guid {
        let mut data4_owned = [0; 8];
        data4_owned.clone_from_slice(&data4[0..8]);
        Guid {
            data1,
            data2,
            data3,
            data4: data4_owned,
        }
    }

    pub fn to_string(&self) -> String {
        format!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7]
        )
    }
}

impl Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}
