# Builds every guest/*.S into a flat binary loaded at guest-physical 0x1000
# (real mode, CS=DS=0). Needs binutils: as, ld.
ORG ?= 0x1000
SRCS := $(wildcard *.S)
BINS := $(SRCS:.S=.bin)

all: $(BINS)

%.o: %.S
	as --32 -o $@ $<

%.bin: %.o
	ld -m elf_i386 -nostdlib -Ttext=$(ORG) --oformat binary -o $@ $<

# Print the bytes as a Rust array, to paste into a module selftest.
%.rs: %.bin
	@echo "const GUEST_CODE: [u8; $$(stat -c %s $<)] = [$$(od -An -v -tx1 $< | sed 's/ \([0-9a-f][0-9a-f]\)/0x\1, /g' | tr -d '\n')];"

clean:
	rm -f *.o *.bin
.PHONY: all clean
