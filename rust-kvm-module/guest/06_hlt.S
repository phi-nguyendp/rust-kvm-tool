# Stage 6 guest: the whole program. Assemble with `as 06_hlt.S -o 06.o && objcopy -O binary
# -j .text 06.o 06.bin`; module/s06_run.rs embeds the same single byte (0xf4) by hand.
        .text
        .code64
        hlt
