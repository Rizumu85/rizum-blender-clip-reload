// Read-only trace for the external-id selector route around RVA 0x331EB72.
//
// Scope:
//   0x331EB72 is the post-call test after an indirect call through [rax+0x1a8].
//   In the live positive-control run that indirect call targeted 0x143A3E180.
//   This trace logs every external id that passes through this selector route.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const REQUESTED_96KB_ID = 'extrnlid5943B673F7C84B779ED2D7C96E942EAE';
const TARGET_VECTOR_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_HASH = 'fnv1a32:7bece4ac';
const REQUESTED_96KB_HASH = 'fnv1a32:b59c35c3';
const PREFIX_BYTES = 96;
const HASH_BYTES = 4096;
const SUMMARY_INTERVAL_MS = 5000;
const MAX_BACKTRACES = 20;

const RVAS = {
  selector_entry: 0x331e9f0,
  selector_type_reader_return: 0x331ea2f,
  selector_external_len_return: 0x331eaeb,
  selector_external_ptr_return: 0x331eb1a,
  selector_indirect_pre_call: 0x331eb6c,
  selector_post_call: 0x331eb72,
  selector_success_payload_len: 0x331eb80,
  selector_success_payload_ptr: 0x331eb8c,
  selector_store_value_call: 0x331eb95,
  selector_fallback_store_call: 0x331ebb3,
  registration_entry: 0x3a3e180,
  registration_leave_post_first: 0x3a3e1ac,
  registration_leave_post_second: 0x3a3e1d6,
  loader_entry: 0x3a41780,
  loader_post_body_size: 0x3a41d58,
  loader_post_allocation: 0x3a41d6d,
  loader_post_body_read: 0x3a41d7f,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_external_id_selector_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let selectorInvocationIndex = 0;
let loaderInvocationIndex = 0;
let backtracesWritten = 0;
const selectorStackByThread = {};
const loaderStackByThread = {};
const counts = {};
const externalIdCounts = {};
const bodySizeCounts = {};
const hashCounts = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function tidKey() { return String(Process.getCurrentThreadId()); }
function vaOfRva(rva) { return cspBase.add(rva); }

function ptrString(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.toString();
  } catch (_) {
    return null;
  }
}

function rvaOf(p) {
  try {
    if (p === null || p.isNull()) return null;
    return '0x' + p.sub(cspBase).toUInt32().toString(16);
  } catch (_) {
    return null;
  }
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function bump(map, key) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  map[k] = (map[k] || 0) + 1;
}

function bumpCount(key) {
  counts[key] = (counts[key] || 0) + 1;
}

function stack(map) {
  const key = tidKey();
  if (!map[key]) map[key] = [];
  return map[key];
}

function activeSelector() {
  const s = stack(selectorStackByThread);
  return s.length ? s[s.length - 1] : null;
}

function activeLoader() {
  const s = stack(loaderStackByThread);
  return s.length ? s[s.length - 1] : null;
}

function readBytes(ptrValue, length) {
  if (!ptrValue || length <= 0) return null;
  try {
    return new Uint8Array(ptrValue.readByteArray(length));
  } catch (_) {
    return null;
  }
}

function toHex(u8, maxBytes) {
  if (!u8) return null;
  const n = Math.min(u8.length, maxBytes || u8.length);
  return Array.from(u8.slice(0, n)).map((b) => b.toString(16).padStart(2, '0')).join(' ');
}

function toAscii(u8, maxBytes) {
  if (!u8) return null;
  const n = Math.min(u8.length, maxBytes || u8.length);
  return Array.from(u8.slice(0, n)).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
}

function readAscii(ptrValue, length) {
  return toAscii(readBytes(ptrValue, length), length);
}

function checksum(u8) {
  if (!u8) return null;
  let h = 0x811c9dc5;
  for (const b of u8) {
    h ^= b;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return 'fnv1a32:' + h.toString(16).padStart(8, '0');
}

function readPrefix(ptrValue, length) {
  const n = Math.min(length || 0, PREFIX_BYTES);
  const u8 = readBytes(ptrValue, n);
  return {
    hex: toHex(u8, n),
    ascii: toAscii(u8, n),
  };
}

function safeReadPointer(p) {
  try { return p.readPointer(); } catch (_) { return null; }
}

function safeReadU32(p) {
  try { return p.readU32(); } catch (_) { return null; }
}

function nearbyFields(p, bytes) {
  if (!p || p.isNull()) return null;
  const rows = [];
  for (let off = 0; off <= bytes; off += 8) {
    rows.push({
      offset: off,
      ptr: ptrString(safeReadPointer(p.add(off))),
      ptr_rva: rvaOf(safeReadPointer(p.add(off))),
      u32: safeReadU32(p.add(off)),
    });
  }
  return rows;
}

function idClass(id) {
  return {
    is_requested_96kb_id: id === REQUESTED_96KB_ID,
    is_target_vector_id: id === TARGET_VECTOR_ID,
  };
}

function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 22)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function rvaMapForJson(values) {
  const result = {};
  for (const key in values) {
    if (Object.prototype.hasOwnProperty.call(values, key)) result[key] = '0x' + values[key].toString(16);
  }
  return result;
}

function emitSelector(stage, context, extra) {
  const rec = activeSelector();
  const row = Object.assign({
    event: stage,
    timestamp_ms: nowMs(),
    process_id: processId,
    thread_id: Process.getCurrentThreadId(),
    selector_invocation_index: rec ? rec.selector_invocation_index : null,
    enclosing_function_rva: '0x331e9f0',
    caller_rva: context ? rvaOf(context.returnAddress || NULL) : null,
    rcx: context ? ptrString(context.rcx) : null,
    rdx: context ? ptrString(context.rdx) : null,
    r8: context ? ptrString(context.r8) : null,
    r9: context ? ptrString(context.r9) : null,
    external_id_ascii: rec ? rec.external_id_ascii : null,
    external_id_length: rec ? rec.external_id_length : null,
    request_object_ptr: rec ? rec.request_object_ptr : null,
    parent_object_candidate: rec ? rec.parent_object_candidate : null,
    resource_owner_candidate: rec ? rec.resource_owner_candidate : null,
  }, extra || {});
  if (rec && rec.external_id_ascii) Object.assign(row, idClass(rec.external_id_ascii));
  writeJson(row);
}

function hookSelector() {
  Interceptor.attach(vaOfRva(RVAS.selector_entry), {
    onEnter(args) {
      const rec = {
        selector_invocation_index: selectorInvocationIndex++,
        thread_id: Process.getCurrentThreadId(),
        caller_rva: rvaOf(this.returnAddress),
        return_address: ptrString(this.returnAddress),
        entry_rcx: ptrString(args[0]),
        entry_rdx: ptrString(args[1]),
        entry_r8: ptrString(args[2]),
        entry_r9: ptrString(args[3]),
        parent_object_candidate: ptrString(args[0]),
        request_object_ptr: ptrString(args[2]),
        resource_owner_candidate: ptrString(args[3]),
        stack_arg_0xe0: ptrString(this.context.rsp.add(0xe0).readPointer()),
        value_type: null,
        external_id_length: null,
        external_id_ptr: null,
        external_id_ascii: null,
      };
      stack(selectorStackByThread).push(rec);
      bumpCount('selector_invocation');
      writeJson({
        event: 'selector_entry',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: rec.thread_id,
        selector_invocation_index: rec.selector_invocation_index,
        enclosing_function_rva: '0x331e9f0',
        caller_rva: rec.caller_rva,
        return_address: rec.return_address,
        rcx: rec.entry_rcx,
        rdx: rec.entry_rdx,
        r8: rec.entry_r8,
        r9: rec.entry_r9,
        request_object_ptr: rec.request_object_ptr,
        parent_object_candidate: rec.parent_object_candidate,
        resource_owner_candidate: rec.resource_owner_candidate,
        stack_arg_0xe0: rec.stack_arg_0xe0,
        request_object_fields_0x80: nearbyFields(args[2], 0x80),
      });
    },
    onLeave(retval) {
      const s = stack(selectorStackByThread);
      const rec = s.length ? s.pop() : null;
      writeJson({
        event: 'selector_leave',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        selector_invocation_index: rec ? rec.selector_invocation_index : null,
        enclosing_function_rva: '0x331e9f0',
        external_id_ascii: rec ? rec.external_id_ascii : null,
        external_id_length: rec ? rec.external_id_length : null,
        return_value: retval.toInt32(),
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.selector_type_reader_return), {
    onEnter() {
      const rec = activeSelector();
      if (!rec) return;
      rec.value_type = this.context.rax.toInt32();
      emitSelector('selector_type_reader_return', this.context, { value_type: rec.value_type });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.selector_external_len_return), {
    onEnter() {
      const rec = activeSelector();
      if (!rec) return;
      rec.external_id_length = this.context.rax.toInt32();
      emitSelector('selector_external_len_return', this.context, { external_id_length: rec.external_id_length });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.selector_external_ptr_return), {
    onEnter() {
      const rec = activeSelector();
      if (!rec) return;
      rec.external_id_ptr = ptrString(this.context.rax);
      rec.external_id_ascii = readAscii(this.context.rax, rec.external_id_length || 0);
      if (rec.external_id_ascii) bump(externalIdCounts, rec.external_id_ascii);
      const rowExtra = {
        external_id_ptr: rec.external_id_ptr,
        external_id_ascii: rec.external_id_ascii,
        external_id_length: rec.external_id_length,
        request_object_fields_0x80: rec.request_object_ptr ? nearbyFields(ptr(rec.request_object_ptr), 0x80) : null,
      };
      emitSelector('selector_external_ptr_return', this.context, rowExtra);
    },
  });

  Interceptor.attach(vaOfRva(RVAS.selector_indirect_pre_call), {
    onEnter() {
      const rec = activeSelector();
      let indirectTarget = null;
      try {
        indirectTarget = this.context.rax.readPointer().add(0x1a8).readPointer();
      } catch (_) {
      }
      const row = {
        indirect_target: ptrString(indirectTarget),
        indirect_target_rva: rvaOf(indirectTarget),
        call_rcx: ptrString(this.context.rcx),
        call_rdx: ptrString(this.context.rdx),
        call_r8: ptrString(this.context.r8),
        call_r9: ptrString(this.context.r9),
      };
      if (rec) {
        row.external_id_ascii = rec.external_id_ascii;
        row.external_id_length = rec.external_id_length;
      }
      const fullRow = Object.assign({
        event: 'selector_indirect_pre_call',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        selector_invocation_index: rec ? rec.selector_invocation_index : null,
        enclosing_function_rva: '0x331e9f0',
        request_object_ptr: rec ? rec.request_object_ptr : null,
        parent_object_candidate: rec ? rec.parent_object_candidate : null,
        resource_owner_candidate: rec ? rec.resource_owner_candidate : null,
      }, row);
      if (rec && rec.external_id_ascii) Object.assign(fullRow, idClass(rec.external_id_ascii));
      addBacktrace(fullRow, this.context);
      writeJson(fullRow);
    },
  });

  Interceptor.attach(vaOfRva(RVAS.selector_post_call), {
    onEnter() {
      const rec = activeSelector();
      const row = {
        event: 'selector_post_call',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        selector_invocation_index: rec ? rec.selector_invocation_index : null,
        enclosing_function_rva: '0x331e9f0',
        success_return: this.context.rax.toInt32(),
        external_id_ascii: rec ? rec.external_id_ascii : null,
        external_id_length: rec ? rec.external_id_length : null,
        request_object_ptr: rec ? rec.request_object_ptr : null,
        parent_object_candidate: rec ? rec.parent_object_candidate : null,
        resource_owner_candidate: rec ? rec.resource_owner_candidate : null,
      };
      if (rec && rec.external_id_ascii) Object.assign(row, idClass(rec.external_id_ascii));
      writeJson(row);
    },
  });

  for (const [rva, name] of [
    [RVAS.selector_success_payload_len, 'selector_success_payload_len'],
    [RVAS.selector_success_payload_ptr, 'selector_success_payload_ptr'],
    [RVAS.selector_store_value_call, 'selector_store_value_call'],
    [RVAS.selector_fallback_store_call, 'selector_fallback_store_call'],
  ]) {
    Interceptor.attach(vaOfRva(rva), {
      onEnter() {
        emitSelector(name, this.context, {
          rax: ptrString(this.context.rax),
          eax: this.context.rax.toInt32(),
        });
      },
    });
  }
}

function hookRegistrationAndLoader() {
  Interceptor.attach(vaOfRva(RVAS.registration_entry), {
    onEnter(args) {
      const active = activeSelector();
      bumpCount('registration_invocation');
      writeJson({
        event: 'registration_entry',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        selector_invocation_index: active ? active.selector_invocation_index : null,
        caller_rva: rvaOf(this.returnAddress),
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: args[3].toUInt32(),
        external_id_ascii: readAscii(args[2], args[3].toUInt32()),
        external_id_length: args[3].toUInt32(),
      });
    },
    onLeave(retval) {
      const active = activeSelector();
      writeJson({
        event: 'registration_leave',
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        selector_invocation_index: active ? active.selector_invocation_index : null,
        return_value: retval.toInt32(),
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_entry), {
    onEnter(args) {
      const active = activeSelector();
      const rec = {
        loader_invocation_index: loaderInvocationIndex++,
        selector_invocation_index: active ? active.selector_invocation_index : null,
        thread_id: Process.getCurrentThreadId(),
        caller_rva: rvaOf(this.returnAddress),
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: args[3].toUInt32(),
        external_id_ascii: readAscii(args[2], args[3].toUInt32()),
        external_id_length: args[3].toUInt32(),
        body_size: null,
        dest_ptr: null,
        dest_hash: null,
      };
      stack(loaderStackByThread).push(rec);
      bumpCount('loader_invocation');
      if (rec.external_id_ascii) bump(externalIdCounts, rec.external_id_ascii);
      writeJson(Object.assign({ event: 'loader_entry', timestamp_ms: nowMs(), process_id: processId }, rec, idClass(rec.external_id_ascii)));
    },
    onLeave(retval) {
      const s = stack(loaderStackByThread);
      const rec = s.length ? s.pop() : null;
      if (!rec) return;
      rec.return_value = retval.toInt32();
      writeJson(Object.assign({ event: 'loader_leave', timestamp_ms: nowMs(), process_id: processId }, rec, idClass(rec.external_id_ascii)));
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_body_size), {
    onEnter() {
      const rec = activeLoader();
      if (!rec) return;
      rec.body_size = this.context.rax.toUInt32();
      bump(bodySizeCounts, rec.body_size);
      writeJson({
        event: 'loader_post_body_size',
        timestamp_ms: nowMs(),
        process_id: processId,
        loader_invocation_index: rec.loader_invocation_index,
        selector_invocation_index: rec.selector_invocation_index,
        external_id_ascii: rec.external_id_ascii,
        body_size: rec.body_size,
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_allocation), {
    onEnter() {
      const rec = activeLoader();
      if (!rec) return;
      rec.dest_ptr = ptrString(this.context.rax);
      writeJson({
        event: 'loader_post_allocation',
        timestamp_ms: nowMs(),
        process_id: processId,
        loader_invocation_index: rec.loader_invocation_index,
        selector_invocation_index: rec.selector_invocation_index,
        external_id_ascii: rec.external_id_ascii,
        body_size: rec.body_size,
        dest_ptr: rec.dest_ptr,
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_body_read), {
    onEnter() {
      const rec = activeLoader();
      if (!rec || !rec.dest_ptr) return;
      const dest = ptr(rec.dest_ptr);
      const prefix = readPrefix(dest, Math.min(PREFIX_BYTES, rec.body_size || PREFIX_BYTES));
      const hashBytes = readBytes(dest, Math.min(HASH_BYTES, rec.body_size || HASH_BYTES));
      rec.dest_hash = checksum(hashBytes);
      bump(hashCounts, rec.dest_hash);
      writeJson(Object.assign({
        event: 'loader_post_body_read',
        timestamp_ms: nowMs(),
        process_id: processId,
        loader_invocation_index: rec.loader_invocation_index,
        selector_invocation_index: rec.selector_invocation_index,
        external_id_ascii: rec.external_id_ascii,
        body_size: rec.body_size,
        dest_ptr: rec.dest_ptr,
        dest_hash: rec.dest_hash,
        prefix_ascii: prefix.ascii,
        prefix_hex: prefix.hex,
      }, idClass(rec.external_id_ascii), {
        is_requested_96kb_hash: rec.dest_hash === REQUESTED_96KB_HASH,
        is_target_vector_hash: rec.dest_hash === TARGET_HASH,
      }));
    },
  });
}

function writeSummary(reason) {
  writeJson({
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    external_id_counts: externalIdCounts,
    body_size_counts: bodySizeCounts,
    hash_counts: hashCounts,
    selector_invocations: counts.selector_invocation || 0,
    registration_invocations: counts.registration_invocation || 0,
    loader_invocations: counts.loader_invocation || 0,
  });
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  requested_96kb_id: REQUESTED_96KB_ID,
  target_vector_id: TARGET_VECTOR_ID,
  hook_rvas: rvaMapForJson(RVAS),
});

hookSelector();
hookRegistrationAndLoader();

writeJson({
  event: 'ready_hooks_installed',
  timestamp_ms: nowMs(),
  process_id: processId,
  output_path: outPath,
});

setInterval(function () { writeSummary('periodic'); }, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeSummary('unload');
    out.flush();
    out.close();
  } catch (_) {
  }
});
