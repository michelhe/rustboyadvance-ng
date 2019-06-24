#[macro_use]
extern crate enum_primitive_derive;
extern crate num_traits;

extern crate bit;

pub mod arm;

pub const REG_PC: usize = 15;

pub fn reg_string(reg: usize) -> &'static str {
    let reg_names = &[
        "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "fp", "ip", "sp",
        "lr", "pc",
    ];
    reg_names[reg]
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
