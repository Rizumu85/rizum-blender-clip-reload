// Read-only owner trace for vector CHNKExta body route.
//
// Scope:
//   0x143A41780 is an external-body lookup/read helper.  It returns status in
//   EAX, not renderer data.  This trace follows the vector external id through
//   function entry, size/allocation/read stages, return, and the two known
//   caller return sites.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_EXT_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_BODY_SIZE = 2644;

const RVAS = {
  function_entry: 0x3a41780,
  post_body_size: 0x3a41d58,
  post_allocation: 0x3a41d6d,
  post_body_read: 0x3a41d7f,
  caller_first_post: 0x3a3e1ac,
  caller_second_post: 0x3a3e1d6,
};

const MAX_PREFIX_BYTES = 96;
const MAX_RETURN_FIELD_BYTES = 0x80;
const SUMMARY_INTERVAL_MS = 5000;

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_exta_vector_body_owner_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let invocationIndex = 0;
const activeByThread = {};
const lastReturnByThread = {};
const counts = {};
const vectorCounts = {};
const callerCounts = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() {
  return Date.now();
}

function tidKey() {
  return String(Process.getCurrentThreadId());
}

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

function vaOfRva(rva) {
  return cspBase.add(rva);
}

function bump(map, key) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  map[k] = (map[k] || 0) + 1;
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function stackForThread() {
  const key = tidKey();
  if (!activeByThread[key]) activeByThread[key] = [];
  return activeByThread[key];
}

function activeRecord() {
  const stack = stackForThread();
  return stack.length ? stack[stack.length - 1] : null;
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
  const u8 = readBytes(ptrValue, length);
  return toAscii(u8, length);
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

function samplePrefix(ptrValue, size) {
  const n = Math.min(size || 0, MAX_PREFIX_BYTES);
  const u8 = readBytes(ptrValue, n);
  return {
    hex: toHex(u8, n),
    ascii: toAscii(u8, n),
    checksum_prefix: checksum(u8),
  };
}

function samplePointerFields(ptrValue) {
  if (!ptrValue || ptrValue.isNull()) return null;
  const fields = [];
  for (let off = 0; off < MAX_RETURN_FIELD_BYTES; off += 8) {
    try {
      const v = ptrValue.add(off).readPointer();
      fields.push({ offset: off, ptr: ptrString(v), rva: rvaOf(v) });
    } catch (_) {
      fields.push({ offset: off, ptr: null, rva: null });
    }
  }
  return fields;
}

function shouldLog(rec) {
  return Boolean(
    rec &&
    (rec.target_external_id_match ||
      rec.body_size === TARGET_BODY_SIZE ||
      rec.does_dest_match_vector_body)
  );
}

function publicRec(rec) {
  return {
    invocation_index: rec.invocation_index,
    thread_id: rec.thread_id,
    caller_rva: rec.caller_rva,
    return_address: rec.return_address,
    entry_args: rec.entry_args,
    candidate_external_id_ascii: rec.candidate_external_id_ascii,
    target_external_id_match: rec.target_external_id_match,
    stream_ptr: rec.stream_ptr,
    body_size: rec.body_size,
    dest_ptr: rec.dest_ptr,
    requested_size: rec.requested_size,
    returned_read_size: rec.returned_read_size,
    dest_prefix_ascii: rec.dest_prefix_ascii,
    dest_prefix_hex: rec.dest_prefix_hex,
    dest_hash: rec.dest_hash,
    does_dest_match_native_ext_body_index_19: rec.does_dest_match_vector_body,
    function_return_value: rec.function_return_value,
    return_object_fields: rec.return_object_fields,
    post_read_direct_call_targets: rec.post_read_direct_call_targets,
  };
}

function makeSummary(reason) {
  const activeDepths = {};
  for (const key of Object.keys(activeByThread)) {
    activeDepths[key] = activeByThread[key].length;
  }
  return {
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    vector_counts: vectorCounts,
    caller_counts: callerCounts,
    active_stack_depths: activeDepths,
  };
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_path: csp.path,
  module_base: cspBase.toString(),
  output_path: outPath,
  target_external_id: TARGET_EXT_ID,
  target_body_size: TARGET_BODY_SIZE,
  hook_rvas: Object.fromEntries(Object.entries(RVAS).map(([k, v]) => [k, '0x' + v.toString(16)])),
});

Interceptor.attach(vaOfRva(RVAS.function_entry), {
  onEnter(args) {
    const rec = {
      invocation_index: invocationIndex++,
      thread_id: Process.getCurrentThreadId(),
      timestamp_ms: nowMs(),
      caller_rva: rvaOf(this.returnAddress),
      return_address: ptrString(this.returnAddress),
      entry_args: {
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: ptrString(args[3]),
        r9_u32: args[3].toUInt32(),
      },
      rbx_context_ptr: ptrString(args[0]),
      r13_destination_owner_ptr: ptrString(args[1]),
      external_id_ptr: ptrString(args[2]),
      external_id_len: args[3].toUInt32(),
      candidate_external_id_ascii: readAscii(args[2], Math.min(args[3].toUInt32(), 0x80)),
      target_external_id_match: false,
      stream_ptr: null,
      body_size: null,
      dest_ptr: null,
      requested_size: null,
      returned_read_size: null,
      dest_prefix_ascii: null,
      dest_prefix_hex: null,
      dest_hash: null,
      does_dest_match_vector_body: false,
      function_return_value: null,
      return_object_fields: null,
      post_read_direct_call_targets: [
        { rva: '0x3a41d80', target_rva: '0x20493f0', arg: 'rsp+0xc0', role: 'cleanup' },
        { rva: '0x3a41d8e', target_rva: '0x20493f0', arg: 'rsp+0xf0', role: 'cleanup' },
      ],
    };
    rec.target_external_id_match = rec.candidate_external_id_ascii === TARGET_EXT_ID;
    bump(counts, 'function_entry');
    bump(callerCounts, rec.caller_rva);
    if (rec.target_external_id_match) bump(vectorCounts, 'entry_external_id_match');
    stackForThread().push(rec);
  },
  onLeave(retval) {
    const stack = stackForThread();
    const rec = stack.length ? stack.pop() : null;
    if (!rec) {
      bump(counts, 'unpaired_function_leave');
      return;
    }
    rec.function_return_value = retval.toUInt32();
    rec.onleave_return_value = ptrString(retval);
    rec.return_object_fields = samplePointerFields(ptr(rec.entry_args.rcx));
    bump(counts, 'function_leave');
    if (rec.target_external_id_match) bump(vectorCounts, 'leave_external_id_match');
    if (rec.body_size === TARGET_BODY_SIZE) bump(vectorCounts, 'leave_body_size_match');
    if (rec.does_dest_match_vector_body) bump(vectorCounts, 'leave_dest_match');
    lastReturnByThread[tidKey()] = rec;
    if (shouldLog(rec)) {
      writeJson({ event: 'vector_owner_function_leave', timestamp_ms: nowMs(), ...publicRec(rec) });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.post_body_size), {
  onEnter() {
    const rec = activeRecord();
    bump(counts, 'post_body_size');
    if (!rec) return;
    rec.body_size = this.context.rax.toUInt32();
    if (rec.body_size === TARGET_BODY_SIZE) bump(vectorCounts, 'body_size_match');
    try {
      rec.stream_ptr = ptrString(ptr(rec.entry_args.rcx).add(0xe0).readPointer());
    } catch (_) {
      rec.stream_ptr = null;
    }
    if (shouldLog(rec)) {
      writeJson({ event: 'vector_owner_post_body_size', timestamp_ms: nowMs(), ...publicRec(rec) });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.post_allocation), {
  onEnter() {
    const rec = activeRecord();
    bump(counts, 'post_allocation');
    if (!rec) return;
    rec.dest_ptr = ptrString(this.context.rax);
    rec.requested_size = this.context.rdi.toUInt32();
    if (rec.requested_size === TARGET_BODY_SIZE) bump(vectorCounts, 'requested_size_match');
    if (shouldLog(rec)) {
      writeJson({ event: 'vector_owner_post_allocation', timestamp_ms: nowMs(), ...publicRec(rec) });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.post_body_read), {
  onEnter() {
    const rec = activeRecord();
    bump(counts, 'post_body_read');
    if (!rec) return;
    rec.returned_read_size = this.context.rax.toString();
    const dest = rec.dest_ptr ? ptr(rec.dest_ptr) : null;
    const size = rec.requested_size || rec.body_size || 0;
    const prefix = samplePrefix(dest, size);
    rec.dest_prefix_ascii = prefix.ascii;
    rec.dest_prefix_hex = prefix.hex;
    const fullBytes = readBytes(dest, Math.min(size, 4096));
    rec.dest_hash = checksum(fullBytes);
    rec.does_dest_match_vector_body =
      rec.target_external_id_match &&
      rec.body_size === TARGET_BODY_SIZE &&
      Boolean(rec.dest_prefix_hex && rec.dest_prefix_hex.startsWith('00 00 00 5c 00 00 00 4c'));
    if (rec.does_dest_match_vector_body) bump(vectorCounts, 'dest_match_vector_body_index_19');
    if (shouldLog(rec)) {
      writeJson({ event: 'vector_owner_post_body_read', timestamp_ms: nowMs(), ...publicRec(rec) });
    }
  },
});

function attachCallerPost(rva, label) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter() {
      bump(counts, 'caller_post_return');
      bump(callerCounts, '0x' + rva.toString(16));
      const rec = lastReturnByThread[tidKey()] || null;
      const row = {
        event: 'vector_owner_caller_post_return',
        timestamp_ms: nowMs(),
        caller_site: label,
        caller_rva: '0x' + rva.toString(16),
        invocation_index: rec ? rec.invocation_index : null,
        correlated: Boolean(rec),
        rax_after_call: ptrString(this.context.rax),
        eax_after_call: this.context.rax.toUInt32(),
        where_result_is_used_static: rva === RVAS.caller_first_post
          ? 'test eax,eax; on success mov [rbx+0x3f4],1'
          : 'test eax,eax; on success shared success path mov [rbx+0x3f4],1',
        next_call_target_static: rva === RVAS.caller_second_post ? '0x143a3c0e0 on failure' : null,
        owner_object_rbx: ptrString(this.context.rbx),
        owner_state_0x100: ptrString(this.context.rbx.add(0x100)),
        owner_state_0x250: ptrString(this.context.rbx.add(0x250)),
        owner_flag_0x3f4: safeReadU32(this.context.rbx.add(0x3f4)),
        correlated_record: rec && shouldLog(rec) ? publicRec(rec) : null,
      };
      if (rec && shouldLog(rec)) writeJson(row);
    },
  });
}

function safeReadU32(p) {
  try {
    return p.readU32();
  } catch (_) {
    return null;
  }
}

attachCallerPost(RVAS.caller_first_post, 'first_owner_slot_0x100');
attachCallerPost(RVAS.caller_second_post, 'second_owner_slot_0x250');

writeJson({
  event: 'ready_hooks_installed',
  timestamp_ms: nowMs(),
  process_id: processId,
  output_path: outPath,
});

setInterval(function () {
  writeJson(makeSummary('periodic'));
}, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeJson(makeSummary('unload'));
    out.flush();
    out.close();
  } catch (_) {
  }
});
