.section ".init"

.global _interrupt_vector
.align 4
.arm

.section ".init.vector"
_interrupt_vector:
    b _reset
    b _not_implemented
    b _not_implemented
    b _not_implemented
    b _not_implemented
    b _not_implemented
    b _not_implemented
    b _not_implemented

.section ".init.text"
.global _reset
_reset:
    mrs     r1, cpsr            @ save the mode bits from CPSR
    bic     r0, r1, #0x1F
    orr     r0, r0, #0x13       @ supervisor
    ldr     sp, =_stack_top
    mov     r0, #0
    mov     r1, #0
    mov     r2, #0
    mov     r3, #0
    mov     r4, #0
    mov     r5, #0
    mov     r6, #0
    mov     r7, #0
    mov     r9, #0
    mov     r10, #0
    mov     r11, #0
    mov     r12, #0
    b       main

_not_implemented:
    b _not_implemented
