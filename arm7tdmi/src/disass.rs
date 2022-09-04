use std::fmt;
use std::marker::PhantomData;

use super::Addr;
use super::InstructionDecoder;

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
    pub fn new(base: Addr, bytes: &'a [u8]) -> Disassembler<'_, D> {
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
    D: InstructionDecoder + fmt::Display,
    <D as InstructionDecoder>::IntType: std::fmt::LowerHex,
{
    type Item = (Addr, String);

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        let addr = self.base + self.pos as Addr;
        let decoded: D = D::decode_from_bytes(&self.bytes[(self.pos as usize)..], addr);
        self.pos += self.word_size;
        line.push_str(&format!(
            "{:8x}:\t{:08x} \t{}",
            addr,
            decoded.get_raw(),
            decoded
        ));

        Some((self.pos as Addr, line))
    }
}
