# Stage 9 guest: real mode. Main code at 0000:1000, IVT entry 0x20 at 0000:0080 pointing to
# the handler at 0000:2000 (the host writes the IVT entry; this file is only the code).
# The host injects vector 0x20 on the very first VM entry (RFLAGS.IF=1, so it is
# deliverable); the handler prints 'X' and IRETs back to the HLT.
# Second program (`spin`, see the selftest) is just `jmp .` and is used to test the
# VMX-preemption timer.
	.code16
	.globl _start
_start:
	hlt
	jmp	_start

	.org 0x1000		# handler at 0x1000 + 0x1000 = 0x2000
handler:
	mov	$0x3f8, %dx
	mov	$'X', %al
	out	%al, %dx
	iret

spin:
	jmp	spin
