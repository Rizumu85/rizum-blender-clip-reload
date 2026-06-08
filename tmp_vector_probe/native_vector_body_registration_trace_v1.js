// Read-only focused trace for registering the vector external body owner slot.
//
// Scope:
//   0x143A3E180 first tries owner slot parent+0x100, then parent+0x250.
//   For Vector_SizePressure the vector external body route succeeds at the
//   first 0x143A41780 call and sets [parent+0x3f4] = 1.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_EXT_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_BODY_SIZE = 2644;

const RVAS = {
  parent_entry: 0x3a3e180,
  first_pre_call: 0x3a3e1a7,
  first_post_call: 0x3a3e1ac,
  success_flag_write: 0x3a3e1b0,
  success_after_flag: 0x3a3e1ba,
  second_pre_call: 0x3a3e1d1,
  second_post_call: 0x3a3e1d6,
  fallback_call: 0x3a3e1df,
  fallback_cleanup_call: 0x3a3e1eb,
  loader_entry: 0x3a41780,
  loader_post_body_size: 0x3a41d58,
  loader_post_allocation: 0x3a41d6d,
  loader_post_body_read: 0x3a41d7f,
};

const MAX_PREFIX_BYTES = 96;
const SUMMARY_INTERVAL_MS = 5000;

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_vector_body_registration_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let parentInvocationIndex = 0;
let loaderInvocationIndex = 0;
let backtracesWritten = 0;
const parentStackByThread = {};
const loaderStackByThread = {};
const counts = {};
const vectorCounts = {};
const vectorParentByThread = {};

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

function bump(map, key) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  map[k] = (map[k] || 0) + 1;
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function stack(map) {
  const key = tidKey();
  if (!map[key]) map[key] = [];
  return map[key];
}

function activeParent() {
  const s = stack(parentStackByThread);
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

function safeReadPointer(p) {
  try { return p.readPointer(); } catch (_) { return null; }
}

function safeReadU32(p) {
  try { return p.readU32(); } catch (_) { return null; }
}

function safePointerFields(basePtr, start, end, destPtrString) {
  if (!basePtr || basePtr.isNull()) return [];
  const rows = [];
  for (let off = start; off <= end; off += 8) {
    let ptrValue = null;
    let u32Value = null;
    try { ptrValue = basePtr.add(off).readPointer(); } catch (_) {}
    try { u32Value = basePtr.add(off).readU32(); } catch (_) {}
    const ptrText = ptrString(ptrValue);
    let nestedDestOffset = null;
    if (destPtrString && ptrValue && !ptrValue.isNull()) {
      for (let nested = 0; nested <= 0x80; nested += 8) {
        const nestedPtr = safeReadPointer(ptrValue.add(nested));
        if (ptrString(nestedPtr) === destPtrString) {
          nestedDestOffset = nested;
          break;
        }
      }
    }
    rows.push({
      offset: off,
      ptr: ptrText,
      ptr_rva: rvaOf(ptrValue),
      u32: u32Value,
      equals_dest_ptr: Boolean(destPtrString && ptrText === destPtrString),
      nested_field_equals_dest_ptr_at: nestedDestOffset,
    });
  }
  return rows;
}

function scanObservedObjects(parent, destPtrString, extraArgs) {
  const parentPtr = parent && parent.parent_ptr ? ptr(parent.parent_ptr) : null;
  const slot100 = parent && parent.slot_0x100 ? ptr(parent.slot_0x100) : null;
  const slot250 = parent && parent.slot_0x250 ? ptr(parent.slot_0x250) : null;
  const rows = {
    parent_0x80_0x140: safePointerFields(parentPtr, 0x80, 0x140, destPtrString),
    parent_0x300_0x430: safePointerFields(parentPtr, 0x300, 0x430, destPtrString),
    slot_0x100_0x0_0x180: safePointerFields(slot100, 0x0, 0x180, destPtrString),
    slot_0x250_0x0_0x180: safePointerFields(slot250, 0x0, 0x180, destPtrString),
    extra_args_0x0_0x100: [],
  };
  for (const argText of extraArgs || []) {
    if (!argText) continue;
    try {
      rows.extra_args_0x0_0x100.push({ object: argText, fields: safePointerFields(ptr(argText), 0x0, 0x100, destPtrString) });
    } catch (_) {}
  }
  return rows;
}

function hasDestHit(scan) {
  const buckets = Object.values(scan || {});
  for (const bucket of buckets) {
    if (Array.isArray(bucket)) {
      for (const row of bucket) {
        if (row.equals_dest_ptr || row.nested_field_equals_dest_ptr_at !== null) return true;
        if (row.fields && hasDestHit({ nested: row.fields })) return true;
      }
    }
  }
  return false;
}

function addBacktrace(row, context) {
  if (backtracesWritten >= 4) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 24)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function makeSummary(reason) {
  return {
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    vector_counts: vectorCounts,
  };
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  target_external_id: TARGET_EXT_ID,
  target_body_size: TARGET_BODY_SIZE,
  hook_rvas: Object.fromEntries(Object.entries(RVAS).map(([k, v]) => [k, '0x' + v.toString(16)])),
});

Interceptor.attach(vaOfRva(RVAS.parent_entry), {
  onEnter(args) {
    const parentPtr = args[0];
    const rec = {
      parent_function_invocation_index: parentInvocationIndex++,
      thread_id: Process.getCurrentThreadId(),
      caller_rva: rvaOf(this.returnAddress),
      return_address: ptrString(this.returnAddress),
      parent_ptr: ptrString(parentPtr),
      parent_rbx: ptrString(parentPtr),
      parent_rdx: ptrString(args[1]),
      external_id_ptr: ptrString(args[2]),
      external_id_length: args[3].toUInt32(),
      external_id_ascii: readAscii(args[2], Math.min(args[3].toUInt32(), 0x80)),
      slot_0x100: ptrString(parentPtr.add(0x100)),
      slot_0x250: ptrString(parentPtr.add(0x250)),
      flag_0x3f4_before: safeReadU32(parentPtr.add(0x3f4)),
      flag_0x3f4_after: null,
      vector_loader: null,
      post_success_calls: [],
      pointer_scan: null,
    };
    bump(counts, 'parent_entry');
    if (rec.external_id_ascii === TARGET_EXT_ID) bump(vectorCounts, 'parent_external_id_match');
    stack(parentStackByThread).push(rec);
  },
  onLeave(retval) {
    const s = stack(parentStackByThread);
    const rec = s.length ? s.pop() : null;
    bump(counts, 'parent_leave');
    if (!rec) return;
    rec.parent_return_value = retval.toUInt32();
    rec.flag_0x3f4_after = safeReadU32(ptr(rec.parent_ptr).add(0x3f4));
    const loader = rec.vector_loader;
    if (loader && loader.target_external_id_match && loader.body_size === TARGET_BODY_SIZE) {
      const dest = loader.dest_ptr;
      rec.pointer_scan = scanObservedObjects(rec, dest, rec.post_success_calls.flatMap((c) => c.args || []));
      rec.pointer_scan_has_dest_hit = hasDestHit(rec.pointer_scan);
      bump(vectorCounts, 'parent_leave_vector');
      writeJson({ event: 'vector_registration_parent_leave', timestamp_ms: nowMs(), ...rec });
    }
  },
});

function parentEvent(name, extra) {
  const parent = activeParent();
  if (!parent) return;
  if (parent.external_id_ascii === TARGET_EXT_ID || (parent.vector_loader && parent.vector_loader.target_external_id_match)) {
    writeJson({ event: name, timestamp_ms: nowMs(), ...extra, parent_snapshot: parent });
  }
}

Interceptor.attach(vaOfRva(RVAS.first_pre_call), {
  onEnter() {
    const parent = activeParent();
    bump(counts, 'first_pre_call');
    if (parent) {
      parent.active_call_site = '0x3a3e1a7';
      parent.active_owner_slot = ptrString(this.context.rcx);
      parent.active_external_id_ascii = readAscii(this.context.r8, Math.min(this.context.r9.toUInt32(), 0x80));
      parentEvent('vector_registration_first_pre_call', {
        owner_slot_ptr: ptrString(this.context.rcx),
        external_id_ascii: parent.active_external_id_ascii,
        external_id_length: this.context.r9.toUInt32(),
      });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.second_pre_call), {
  onEnter() {
    const parent = activeParent();
    bump(counts, 'second_pre_call');
    if (parent) {
      parent.active_call_site = '0x3a3e1d1';
      parent.active_owner_slot = ptrString(this.context.rcx);
      parent.active_external_id_ascii = readAscii(this.context.r8, Math.min(this.context.r9.toUInt32(), 0x80));
      parentEvent('vector_registration_second_pre_call', {
        owner_slot_ptr: ptrString(this.context.rcx),
        external_id_ascii: parent.active_external_id_ascii,
        external_id_length: this.context.r9.toUInt32(),
      });
    }
  },
});

function attachPostCall(rva, name) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter() {
      const parent = activeParent();
      bump(counts, name);
      if (!parent) return;
      const success = this.context.rax.toUInt32() !== 0;
      parent.last_loader_success_eax = this.context.rax.toUInt32();
      parent.last_post_call_rva = '0x' + rva.toString(16);
      if (success && parent.vector_loader && parent.vector_loader.target_external_id_match) {
        bump(vectorCounts, name + '_vector_success');
        parentEvent('vector_registration_' + name, {
          success_return_eax: this.context.rax.toUInt32(),
          owner_slot_ptr: parent.active_owner_slot,
        });
      }
    },
  });
}

attachPostCall(RVAS.first_post_call, 'first_post_call');
attachPostCall(RVAS.second_post_call, 'second_post_call');

Interceptor.attach(vaOfRva(RVAS.success_flag_write), {
  onEnter() {
    const parent = activeParent();
    bump(counts, 'success_flag_write');
    if (!parent) return;
    parent.flag_0x3f4_before_write = safeReadU32(this.context.rbx.add(0x3f4));
    if (parent.vector_loader && parent.vector_loader.target_external_id_match) {
      bump(vectorCounts, 'success_flag_write_vector');
      parentEvent('vector_registration_success_flag_write', {
        instruction_rva: '0x3a3e1b0',
        flag_0x3f4_before_write: parent.flag_0x3f4_before_write,
      });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.success_after_flag), {
  onEnter() {
    const parent = activeParent();
    bump(counts, 'success_after_flag');
    if (!parent) return;
    parent.flag_0x3f4_after_write = safeReadU32(this.context.rbx.add(0x3f4));
    if (parent.vector_loader && parent.vector_loader.target_external_id_match) {
      bump(vectorCounts, 'success_after_flag_vector');
      parentEvent('vector_registration_success_after_flag', {
        instruction_rva: '0x3a3e1ba',
        flag_0x3f4_after_write: parent.flag_0x3f4_after_write,
      });
    }
  },
});

function attachPostSuccessCall(rva, label) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      const parent = activeParent();
      bump(counts, label);
      if (!parent) return;
      const row = {
        rva: '0x' + rva.toString(16),
        label,
        args: [ptrString(args[0]), ptrString(args[1]), ptrString(args[2]), ptrString(args[3])],
      };
      parent.post_success_calls.push(row);
      if (parent.vector_loader && parent.vector_loader.target_external_id_match) {
        writeJson({ event: 'vector_registration_post_success_call', timestamp_ms: nowMs(), ...row });
      }
    },
  });
}

attachPostSuccessCall(RVAS.fallback_call, 'fallback_0x143a3c0e0');
attachPostSuccessCall(RVAS.fallback_cleanup_call, 'fallback_cleanup_0x142055b70');

Interceptor.attach(vaOfRva(RVAS.loader_entry), {
  onEnter(args) {
    const parent = activeParent();
    const rec = {
      loader_invocation_index: loaderInvocationIndex++,
      parent_function_invocation_index: parent ? parent.parent_function_invocation_index : null,
      thread_id: Process.getCurrentThreadId(),
      caller_rva: rvaOf(this.returnAddress),
      owner_slot_ptr: ptrString(args[0]),
      r13_destination_owner_ptr: ptrString(args[1]),
      external_id_ptr: ptrString(args[2]),
      external_id_length: args[3].toUInt32(),
      external_id_ascii: readAscii(args[2], Math.min(args[3].toUInt32(), 0x80)),
      target_external_id_match: false,
      body_size: null,
      dest_ptr: null,
      dest_hash: null,
      dest_prefix_hex: null,
      dest_prefix_ascii: null,
      success_return_eax: null,
    };
    rec.target_external_id_match = rec.external_id_ascii === TARGET_EXT_ID;
    bump(counts, 'loader_entry');
    if (rec.target_external_id_match) {
      bump(vectorCounts, 'loader_external_id_match');
      if (parent) parent.vector_loader = rec;
    }
    stack(loaderStackByThread).push(rec);
  },
  onLeave(retval) {
    const s = stack(loaderStackByThread);
    const rec = s.length ? s.pop() : null;
    bump(counts, 'loader_leave');
    if (!rec) return;
    rec.success_return_eax = retval.toUInt32();
    if (rec.target_external_id_match) {
      bump(vectorCounts, 'loader_leave_vector');
      const parent = activeParent();
      if (parent) parent.vector_loader = rec;
      writeJson({ event: 'vector_registration_loader_leave', timestamp_ms: nowMs(), ...rec });
    }
  },
});

Interceptor.attach(vaOfRva(RVAS.loader_post_body_size), {
  onEnter() {
    const rec = activeLoader();
    bump(counts, 'loader_post_body_size');
    if (!rec) return;
    rec.body_size = this.context.rax.toUInt32();
    if (rec.target_external_id_match && rec.body_size === TARGET_BODY_SIZE) bump(vectorCounts, 'loader_body_size_match');
  },
});

Interceptor.attach(vaOfRva(RVAS.loader_post_allocation), {
  onEnter() {
    const rec = activeLoader();
    bump(counts, 'loader_post_allocation');
    if (!rec) return;
    rec.dest_ptr = ptrString(this.context.rax);
    rec.requested_size = this.context.rdi.toUInt32();
  },
});

Interceptor.attach(vaOfRva(RVAS.loader_post_body_read), {
  onEnter() {
    const rec = activeLoader();
    bump(counts, 'loader_post_body_read');
    if (!rec) return;
    const dest = rec.dest_ptr ? ptr(rec.dest_ptr) : null;
    const size = rec.requested_size || rec.body_size || 0;
    const prefixBytes = readBytes(dest, Math.min(size, MAX_PREFIX_BYTES));
    rec.dest_prefix_hex = toHex(prefixBytes, MAX_PREFIX_BYTES);
    rec.dest_prefix_ascii = toAscii(prefixBytes, MAX_PREFIX_BYTES);
    rec.dest_hash = checksum(readBytes(dest, Math.min(size, 4096)));
    rec.dest_matches_vector_body =
      rec.target_external_id_match &&
      rec.body_size === TARGET_BODY_SIZE &&
      Boolean(rec.dest_prefix_hex && rec.dest_prefix_hex.startsWith('00 00 00 5c 00 00 00 4c'));
    if (rec.dest_matches_vector_body) {
      bump(vectorCounts, 'loader_dest_match');
      const row = { event: 'vector_registration_loader_post_body_read', timestamp_ms: nowMs(), ...rec };
      addBacktrace(row, this.context);
      writeJson(row);
      const parent = activeParent();
      if (parent) parent.vector_loader = rec;
    }
  },
});

writeJson({ event: 'ready_hooks_installed', timestamp_ms: nowMs(), process_id: processId, output_path: outPath });

setInterval(function () {
  writeJson(makeSummary('periodic'));
}, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeJson(makeSummary('unload'));
    out.flush();
    out.close();
  } catch (_) {}
});
