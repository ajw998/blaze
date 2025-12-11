use anyhow::Result;
use bincode::config;
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

/// Read a single length-prefixed bincode message from `reader`.
///
/// Wire format:
///   - 4-byte big-endian length (u32)
///   - that many bytes of bincode payload
pub fn read_message<R, T>(reader: &mut R) -> Result<T>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    let (msg, _bytes_read): (T, usize) =
        bincode::serde::decode_from_slice(&buf, config::standard())?;
    Ok(msg)
}

/// Write a single length-prefixed bincode message to `writer`.
///
/// Wire format:
///   - 4-byte big-endian length (u32)
///   - bincode payload
pub fn write_message<W, T>(writer: &mut W, msg: &T) -> Result<()>
where
    W: Write,
    T: Serialize,
{
    let bytes = bincode::serde::encode_to_vec(msg, config::standard())?;
    let len: u32 = bytes
        .len()
        .try_into()
        .expect("message too large to fit into u32 length prefix");

    let len_buf = len.to_be_bytes();
    writer.write_all(&len_buf)?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
