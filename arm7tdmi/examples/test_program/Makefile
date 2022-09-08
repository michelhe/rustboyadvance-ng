# Easiest to build this using `docker run -it --rm -v $(pwd):/code -w /code devkitpro/devkitarm:latest make
TOOLCHAIN := ${DEVKITARM}/bin/arm-none-eabi
CC := ${TOOLCHAIN}-gcc
OBJCOPY := ${TOOLCHAIN}-objcopy
OBJDUMP := ${TOOLCHAIN}-objdump

TARGET := test

CFLAGS := -O0 -g -std=c11 -march=armv4t -mthumb 
LDFLAGS := -Wl,-static


SOURCES := $(wildcard *.[cs])
OBJECTS := $(filter %.o,$(SOURCES:%.c=%.o))
OBJECTS += $(filter %.o,$(SOURCES:%.s=%.o))

all: ${TARGET}.bin objdump

${OBJECTS}: ${SOURCES}
	${CC} -c ${CFLAGS} $^

${TARGET}.elf ${TARGET}.map: ${OBJECTS} ${TARGET}.ld
	${CC} ${LDFLAGS} -Wl,-Map=${TARGET}.map -Wl,-T${TARGET}.ld ${OBJECTS} -o $@
	
${TARGET}.bin: ${TARGET}.elf
	$(OBJCOPY) -v -O binary $^ $@

objdump: ${TARGET}.elf
	${OBJDUMP} -d $^

clean:
	rm -f ${TARGET}.elf
	rm -f ${TARGET}.bin
	rm -f ${TARGET}.map
	rm -f *.o

.PHONY: all clean
