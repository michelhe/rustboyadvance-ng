ENTRY(_reset)

MEMORY {
	RAM : ORIGIN = 0x00000000, LENGTH = 16K
}

SECTIONS {
	. = ORIGIN(RAM);

	.init : {
		*(.init.vector);
		*(.init*);
	} > RAM

	.text : ALIGN(4) {
		*(.text)
        *(.text.*)
	} > RAM
	.data : ALIGN(4) {
		*(.rodata*);
		*(.data*);
	} > RAM
	.bss : ALIGN(4) {
		*(.bss*)
	} > RAM

	.stack : AT (0x00003000) {
		_stack_bottom = .;
		. += 0xfe0;
		_stack_top = .;
	} > RAM
}