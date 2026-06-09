# Target Body Ownership Audit

Target id:

`extrnlid62D15CB4395245648869B4AEBAD8FBCE`

Target body:

- size: `2644`
- hash: `fnv1a32:7bece4ac`
- saved vector flags: `0x2081`

## 0x143A41780 Ownership Re-Audit

- Function body already bracketed by the previous audit as the external body owner/read function.
- Entry contract from caller `0x143A3E180`:
  - first call passes `rcx = parent + 0x100`
  - `rdx = external/resource argument`
  - `r8 = caller rbp`
  - `r9d = caller esi`
- In `0x143A41780`, `rbx` is set from entry `rcx` and is the owner/state object.
- The target external-id compare/read sequence is:
  - `0x143A41D23 call 0x1420575A0` reads the `0x28` byte external id to stack.
  - `0x143A41D43 call 0x1420590D0` compares it with the requested id.
  - `0x143A41D48 test eax,eax`; success continues to body read.
- Target body read sequence:
  - `0x143A41D53 call 0x142057B70` returns body size in `rax`.
  - `0x143A41D58 mov rdi, rax`.
  - `0x143A41D63 mov edx, edi`.
  - `0x143A41D65 mov rcx, r13`.
  - `0x143A41D68 call 0x142056880`.
  - `0x143A41D6D mov r8d, edi`.
  - `0x143A41D70 mov rdx, rax`.
  - `0x143A41D73 mov rcx, [rbx+0xe0]`.
  - `0x143A41D7A call 0x1420575A0`.
  - `0x143A41D7F nop` is the safe post body-read point.
- Therefore `0x1420575A0` receives `rdx = dest_ptr` returned by `0x142056880` and writes the body bytes directly into that buffer.
- Calls after the body read and before return on success are cleanup only:
  - `0x143A41D80 lea rcx, [rsp+0xc0]`; `0x143A41D88 call 0x1420493F0`
  - `0x143A41D8E lea rcx, [rsp+0xf0]`; `0x143A41D96 call 0x1420493F0`
  - `0x143A41D9B mov eax, 1`; `0x143A41DA0 jmp 0x143A41DC0`
- Static reading: the cleanup calls consume temporary stack wrappers, not the final body owner. They do not receive `dest_ptr` in the visible register setup.

Earlier stores inside `0x143A41780` matter for ownership:

- `0x143A41BF8 mov [rbx+0xe0], rax`
- `0x143A41C06 mov [rbx+0xe8], rcx`

Those install stream/wrapper state in the entry owner object before body selection. The target body destination itself is allocated through `r13` later, not visibly written to `[rbx+...]` by a direct instruction in the post-read tail.

## 0x142056880 Allocation/Reserve Helper

- Function start: `0x142056880`.
- Function end: `0x142056A46`.
- Argument contract from the target caller:
  - `rcx = r13`, a blob/vector/string-like owner object.
  - `edx = requested byte size`.
  - `r8/r9` are not used by the visible reserve contract on this path.
- Key fields:
  - `[owner+0x08]`: heap/external buffer pointer, if non-null.
  - `[owner+0x18]`: logical size, written from requested size.
  - `[owner+0x1c]`: capacity/reserved size.
  - `[owner+0x20]`: inline buffer used when `[owner+0x08] == 0`.
- If requested size exceeds capacity:
  - `0x1438CAA0C` allocates raw memory.
  - `0x142055540` wraps the allocation in a ref-counted temp.
  - old bytes are copied with `0x1420590C0`.
  - `0x14204A220(owner, temp, requested_size)` installs the temp into the owner.
- `0x14204A220` does the ownership assignment:
  - `0x14204A23C call 0x1401985C0` with `rcx = owner+8`, `rdx = temp`, swapping/installing the ref-counted allocation wrapper.
  - `0x14204A241 mov [owner+0x1c], requested_capacity`.
  - releases the old temp wrapper from `[temp+8]` if present.
- After reserve:
  - `0x142056957 mov [owner+0x18], requested_size`.
  - if `[owner+0x08] != 0`, `0x142056A28 mov rax, [owner+0x08]`.
  - otherwise `0x142056A2E lea rax, [owner+0x20]`.
- Static conclusion: `0x142056880` returns the writable body buffer pointer. It is either the heap buffer stored at `[r13+0x08]` or the inline buffer at `[r13+0x20]`. For a 2644-byte vector body, the heap path is expected, so the primary owner candidate is `r13` field `+0x08`, with size/capacity at `+0x18/+0x1c`.

## 0x1420575A0 Body Read Helper

- Function start: `0x1420575A0`.
- It receives:
  - `rcx = stream object`.
  - `rdx = destination pointer`.
  - `r8d = byte count`.
- On direct/normal paths it copies into `rdx` / `r12`, including via virtual read/copy calls and `0x1438CBAC0`.
- In the target sequence, `rdx` is exactly the `dest_ptr` returned by `0x142056880`.
- Static conclusion: body bytes are written directly into `dest_ptr`; there is no visible immediate parser call between body read and return.

## Open Dynamic Question

Static ownership is likely:

`r13 + 0x08 = dest_ptr`, `r13 + 0x18 = 2644`, `r13 + 0x1c >= 2644`.

But static disassembly alone does not prove whether `r13` is stored into `rbx`, a parent field, or a wrapper/cache before return. The target-only ownership trace is required to confirm whether the final classification is:

- A: direct owner field,
- B: blob/vector-like struct owned by `r13/rdx`,
- C: copied before return,
- D: passed to parser before return,
- E: registered into cache/map,
- F: no observed owner.

Trace:

`tmp_vector_probe/native_target_body_ownership_trace_v1.js`

Correlator:

`tools/correlate_target_body_ownership.py`
