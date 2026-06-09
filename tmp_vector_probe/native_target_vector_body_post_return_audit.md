# Target VectorData Body Post-Return Audit

Target id:

`extrnlid62D15CB4395245648869B4AEBAD8FBCE`

## 0x143A3E180 Wrapper

- Function start: `0x143A3E180` / RVA `0x3A3E180`.
- Function end: `0x143A3E20D` (`ret`).
- First loader call:
  - `0x143A3E19A add rcx, 0x100`
  - `0x143A3E1A1 mov rbp, r8`
  - `0x143A3E1A4 mov rdi, rdx`
  - `0x143A3E1A7 call 0x143A41780`
- Post-call/test site:
  - `0x143A3E1AC test eax, eax`
  - `0x143A3E1AE je 0x143A3E1C1`
- First-call success branch:
  - `0x143A3E1B0 mov dword [rbx + 0x3f4], 1`
  - `0x143A3E1BA mov eax, 1`
  - `0x143A3E1BF jmp 0x143A3E1F9`
  - No direct parser/copy/decompressor call occurs on this success branch.
- First-call failure branch:
  - `0x143A3E1C1 lea rcx, [rbx + 0x250]`
  - `0x143A3E1C8 mov r9d, esi`
  - `0x143A3E1CB mov r8, rbp`
  - `0x143A3E1CE mov rdx, rdi`
  - `0x143A3E1D1 call 0x143A41780`
  - `0x143A3E1D6 test eax, eax`
  - `0x143A3E1D8 jne 0x143A3E1BA`
- Full failure fallback:
  - `0x143A3E1DA mov edx, esi`
  - `0x143A3E1DC mov rcx, rbp`
  - `0x143A3E1DF call 0x143A3C0E0`
  - if that succeeds, `0x143A3E1E8 mov rcx, rdi`; `0x143A3E1EB call 0x142055B70`.

Static implication: `0x143A3E1AC` is only the first loader post-call/test site. For a successful first-slot target load, the wrapper only sets `parent+0x3f4 = 1` and returns success. The 2644-byte body must be owned/stored inside `0x143A41780` through the first argument object `parent+0x100`, or through an object reachable from that owner.

## 0x143A41780 Target Body Read Core

Relevant internal sequence:

- `0x143A41CFD call 0x142057B70`
- `0x143A41D09 call 0x142057B70`
- `0x143A41D0E cmp rax, 0x28`
- `0x143A41D23 call 0x1420575A0`
- `0x143A41D43 call 0x1420590D0`
- `0x143A41D48 test eax, eax`
- `0x143A41D53 call 0x142057B70` reads body size.
- `0x143A41D58 mov rdi, rax`
- `0x143A41D68 call 0x142056880` allocates/reserves destination.
- `0x143A41D6D mov r8d, edi`
- `0x143A41D70 mov rdx, rax`
- `0x143A41D7A call 0x1420575A0` reads body payload.
- `0x143A41D7F nop` is the safe post-read probe point.
- Success then cleans temporary stack objects and returns `eax=1`.

Earlier in the same function, `0x143A41BF8 mov [rbx+0xe0], rax` and `0x143A41C06 mov [rbx+0xe8], rcx` install stream/wrapper state into the owner object. The target trace should therefore diff `parent`, `parent+0x100`, and allocator/owner candidates such as `r13` around the successful `0x143A41780` invocation.

## Trace Focus

The next diagnostic should not hook renderer code. It should answer whether the target 2644-byte body is:

- stored directly in the owner object,
- stored in a wrapper/cache object reachable from the owner,
- passed to a parser after the wrapper returns,
- copied/decompressed into another buffer,
- or left only in temporary owner state.

The created trace is:

`tmp_vector_probe/native_target_vector_body_post_return_trace_v1.js`

The correlator is:

`tools/correlate_target_vector_body_post_return.py`
