# Stage 10 guest: exercises the I/O and MSR bitmaps. Expected exits, in order:
#   out 0x80   -> NO exit (port passed through, harmless POST-code port)
#   out 0x3f8  -> exit (EXIT_REASON_IO_INSTRUCTION, port 0x3f8, value 'A')
#   rdmsr 0x174 (IA32_SYSENTER_CS)  -> NO exit (read passed through)
#   rdmsr 0xc0000080 (EFER)         -> exit (EXIT_REASON_MSR_READ, ecx=0xc0000080)
#   hlt        -> exit (EXIT_REASON_HLT)
# Real mode, loaded at GPA 0x1000.
	.code16
	.globl _start
_start:
	mov	$0x00, %al
	out	%al, $0x80
	mov	$0x41, %al
	mov	$0x3f8, %dx
	out	%al, %dx
	mov	$0x174, %ecx
	rdmsr
	mov	$0xc0000080, %ecx
	rdmsr
	hlt
