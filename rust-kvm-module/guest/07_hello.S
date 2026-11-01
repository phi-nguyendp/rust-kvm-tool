# Stage 7/11 guest: print a string on COM1 (port 0x3f8), then halt.
# Real mode, loaded at GPA 0x1000, entered with RIP=0x1000, CS=DS=0 (base 0).
# Every `out` is a VM exit (stage 7: EXIT_REASON_IO_INSTRUCTION); `hlt` is the last.
	.code16
	.globl _start
_start:
	cld
	mov	$msg, %si
	mov	$0x3f8, %dx
1:	lodsb
	test	%al, %al
	jz	2f
	out	%al, %dx
	jmp	1b
2:	hlt
	jmp	2b
msg:	.asciz "Hello from the guest\n"
