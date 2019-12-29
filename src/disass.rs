use std::marker::PhantomData;

use super::core::arm7tdmi::{InstructionDecoder, InstructionDecoderError};
use super::core::Addr;
use std::io::ErrorKind;

pub struct Disassembler<'a, D>
where
    D: InstructionDecoder,
{
    base: Addr,
    pos: usize,
    bytes: &'a [u8],
    pub word_size: usize,
    instruction_decoder: PhantomData<D>,
}

impl<'a, D> Disassembler<'a, D>
where
    D: InstructionDecoder,
{
    pub fn new(base: Addr, bytes: &'a [u8]) -> Disassembler<D> {
        Disassembler {
            base: base as Addr,
            pos: 0,
            bytes: bytes,
            word_size: std::mem::size_of::<D::IntType>(),
            instruction_decoder: PhantomData,
        }
    }
}

impl<'a, D> Iterator for Disassembler<'a, D>
where
    D: InstructionDecoder,
    <D as InstructionDecoder>::IntType: std::fmt::LowerHex,
{
    type Item = (Addr, String);

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        let addr = self.base + self.pos as Addr;
        let decoded: Option<D> =
            match D::decode_from_bytes(&self.bytes[(self.pos as usize)..], addr) {
                Ok(decoded) => {
                    self.pos += self.word_size;
                    Some(decoded)
                }
                Err(InstructionDecoderError::IoError(ErrorKind::UnexpectedEof)) => {
                    return None;
                }
                _ => {
                    self.pos += self.word_size;
                    None
                }
            };

        match decoded {
            Some(insn) => {
                line.push_str(&format!("{:8x}:\t{:08x} \t{}", addr, insn.get_raw(), insn))
            }
            _ => line.push_str(&format!("{:8x}:\t \t<UNDEFINED>", addr)),
        };

        Some((self.pos as Addr, line))
    }
}
