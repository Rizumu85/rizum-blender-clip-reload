// Read-only target VectorData external body post-return trace.
//
// Scope:
// - target id only: extrnlid62D15CB4395245648869B4AEBAD8FBCE
// - external body owner path only: 0x143A3E180 -> 0x143A41780
// - no renderer hooks, no row-span hooks, no memory patches.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_BODY_SIZE = 2644;
const TARGET_HASH = 'fnv1a32:7bece4ac';
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  registration_entry: 0x3a3e180,
  registration_first_pre_call: 0x3a3e1a7,
  registration_first_post_call: 0x3a3e1ac,
  registration_first_success_store: 0x3a3e1b0,
  registration_success_done: 0x3a3e1ba,
  registration_second_pre_call: 0x3a3e1d1,
  registration_second_post_call: 0x3a3e1d6,
  registration_fallback_parser: 0x3a3e1df,
  registration_fallback_cleanup: 0x3a3e1eb,

  loader_entry: 0x3a41780,
  loader_id_check_post: 0x3a41d48,
  loader_post_body_size: 0x3a41d58,
  loader_post_allocation: 0x3a41d6d,
  loader_post_body_read: 0x3a41d7f,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_target_vector_body_post_return_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let loaderInvocationIndex = 0;
let backtracesWritten = 0;
const counts = {};
const targetCounts = {};
const callerCounts = {};

const registrationStackByThread = {};
const loaderStackByThread = {};
const callerPreByThread = {};
const lastLoaderByThread = {};
const targetRecords = [];

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function tidKey() { return String(Process.getCurrentThreadId()); }
function vaOfRva(rva) { return cspBase.add(rva); }
function ptrString(p) {
  try { return (!p || p.isNull()) ? null : p.toString(); } catch (_) { return null; }
}
function rvaOf(p) {
  try {
    if (!p || p.isNull()) return null;
    const delta = p.sub(cspBase);
    if (delta.compare(ptr(0)) < 0 || delta.compare(ptr(0x8000000)) > 0) return null;
    return `0x${delta.toUInt32().toString(16)}`;
  } catch (_) { return null; }
}
function bump(map, key) { const k = key || 'unknown'; map[k] = (map[k] || 0) + 1; }
function pushMapStack(map, key, value) {
  if (!map[key]) map[key] = [];
  map[key].push(value);
}
function peekMapStack(map, key) {
  const stack = map[key] || [];
  return stack.length ? stack[stack.length - 1] : null;
}
function popMapStack(map, key) {
  const stack = map[key] || [];
  if (!stack.length) return null;
  const value = stack.pop();
  if (!stack.length) delete map[key];
  return value;
}
function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}
function logEvent(event, details, context, forceBacktrace) {
  bump(counts, event);
  if (details && details.is_target) bump(targetCounts, event);
  if (details && details.caller_rva) bump(callerCounts, details.caller_rva);
  const row = {
    event,
    timestamp_ms: nowMs(),
    event_index: eventIndex++,
    process_id: processId,
    thread_id: Process.getCurrentThreadId(),
    ...details,
  };
  if ((forceBacktrace || (details && details.is_target)) && context && backtracesWritten < 40) {
    backtracesWritten++;
    try {
      row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE).slice(0, 24)
        .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
    } catch (_) { row.backtrace = []; }
  }
  writeJson(row);
}

function safeByteArray(p, size) {
  try {
    if (!p || p.isNull() || !size || size < 0) return null;
    return new Uint8Array(p.readByteArray(size));
  } catch (_) { return null; }
}
function safeAsciiFixed(p, size) {
  const bytes = safeByteArray(p, size);
  if (!bytes) return null;
  let s = '';
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i];
    s += (b >= 0x20 && b <= 0x7e) ? String.fromCharCode(b) : '.';
  }
  return s;
}
function safeUtf8(p, maxLen) {
  try { if (!p || p.isNull()) return null; return p.readUtf8String(maxLen || 256); }
  catch (_) { try { return p.readCString(maxLen || 256); } catch (_2) { return null; } }
}
function safeU32(p) { try { return p.readU32(); } catch (_) { return null; } }
function safePointerAt(p) { try { return (!p || p.isNull()) ? null : p.readPointer(); } catch (_) { return null; } }
function hexPrefix(p, size) {
  const bytes = safeByteArray(p, size);
  if (!bytes) return null;
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join(' ');
}
function fnv1a32(p, size) {
  const bytes = safeByteArray(p, size);
  if (!bytes) return null;
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    h ^= bytes[i];
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return `fnv1a32:${h.toString(16).padStart(8, '0')}`;
}

function snapshotFields(base, size) {
  if (!base || base.isNull()) return null;
  const fields = [];
  for (let off = 0; off < size; off += 8) {
    const addr = base.add(off);
    let pval = null;
    let u32 = null;
    try { pval = addr.readPointer().toString(); } catch (_) {}
    try { u32 = addr.readU32(); } catch (_) {}
    fields.push({
      offset: off,
      offset_hex: `0x${off.toString(16)}`,
      ptr: pval,
      u32,
    });
  }
  return {
    base: ptrString(base),
    size,
    fields,
  };
}

function diffSnapshots(before, after, destPtr, bodySize) {
  if (!before || !after) return null;
  const beforeByOffset = {};
  for (const f of before.fields || []) beforeByOffset[f.offset] = f;
  const changed = [];
  const fieldsEqualDest = [];
  const fieldsEqualBodySize = [];
  for (const f of after.fields || []) {
    const b = beforeByOffset[f.offset] || {};
    const changedPtr = b.ptr !== f.ptr;
    const changedU32 = b.u32 !== f.u32;
    if (changedPtr || changedU32) {
      changed.push({
        offset_hex: f.offset_hex,
        before_ptr: b.ptr,
        after_ptr: f.ptr,
        before_u32: b.u32,
        after_u32: f.u32,
      });
    }
    if (destPtr && f.ptr === destPtr) fieldsEqualDest.push(f.offset_hex);
    if (bodySize !== null && bodySize !== undefined && f.u32 === bodySize) fieldsEqualBodySize.push(f.offset_hex);
  }
  return { changed, fields_equal_dest_ptr: fieldsEqualDest, fields_equal_body_size: fieldsEqualBodySize };
}

function fieldsContainingDest(snapshot, destPtr) {
  if (!snapshot || !destPtr) return [];
  const outRows = [];
  for (const f of snapshot.fields || []) {
    if (!f.ptr || f.ptr === '0x0') continue;
    let nested = null;
    try { nested = ptr(f.ptr); } catch (_) { continue; }
    for (let off = 0; off < 0x200; off += 8) {
      try {
        const pval = nested.add(off).readPointer().toString();
        if (pval === destPtr) {
          outRows.push({
            owner_offset_hex: f.offset_hex,
            nested_base: f.ptr,
            nested_offset_hex: `0x${off.toString(16)}`,
          });
        }
      } catch (_) {
        break;
      }
    }
  }
  return outRows;
}

function regs(context) {
  return {
    rax: ptrString(context.rax),
    rbx: ptrString(context.rbx),
    rcx: ptrString(context.rcx),
    rdx: ptrString(context.rdx),
    r8: ptrString(context.r8),
    r9: ptrString(context.r9),
    r12: ptrString(context.r12),
    r13: ptrString(context.r13),
    r15: ptrString(context.r15),
    rsp: ptrString(context.rsp),
    caller_rva: rvaOf(this && this.returnAddress ? this.returnAddress : ptr(0)),
  };
}

function isTargetRecord(rec) {
  return Boolean(rec && (
    rec.external_id_ascii === TARGET_ID ||
    rec.body_size === TARGET_BODY_SIZE ||
    rec.dest_hash === TARGET_HASH
  ));
}

function currentLoader() {
  return peekMapStack(loaderStackByThread, tidKey());
}

function summarizeRecord(rec) {
  if (!rec) return null;
  return {
    invocation_index: rec.invocation_index,
    caller_rva: rec.caller_rva,
    slot: rec.slot,
    owner_ptr: rec.owner_ptr,
    parent_ptr: rec.parent_ptr,
    external_id_ascii: rec.external_id_ascii,
    external_id_length: rec.external_id_ascii ? rec.external_id_ascii.length : null,
    body_size: rec.body_size,
    dest_ptr: rec.dest_ptr,
    dest_hash: rec.dest_hash,
    body_prefix_hex: rec.body_prefix_hex,
    body_prefix_ascii: rec.body_prefix_ascii,
    return_eax: rec.return_eax,
    loader_return_rax: rec.loader_return_rax,
    fields_equal_dest_ptr: rec.fields_equal_dest_ptr,
    fields_equal_body_size: rec.fields_equal_body_size,
    nested_fields_containing_dest: rec.nested_fields_containing_dest,
  };
}

function emitTargetRecord(event, rec, context, extra) {
  if (!isTargetRecord(rec)) return;
  rec.is_target = true;
  logEvent(event, {
    is_target: true,
    target_id: TARGET_ID,
    target_hash: TARGET_HASH,
    record: summarizeRecord(rec),
    ...extra,
  }, context, true);
}

function installRvaHook(name, rva, callbacks) {
  try {
    const va = vaOfRva(rva);
    Interceptor.attach(va, callbacks(va));
    logEvent('hook_installed', { name, rva: `0x${rva.toString(16)}`, va: va.toString() }, null, false);
  } catch (e) {
    logEvent('hook_install_failed', { name, rva: `0x${rva.toString(16)}`, error: String(e) }, null, false);
  }
}

installRvaHook('registration_entry', RVAS.registration_entry, () => ({
  onEnter(args) {
    const tid = tidKey();
    const rec = {
      parent_ptr: ptrString(args[0]),
      external_arg_ptr: ptrString(args[1]),
      arg_r8: ptrString(args[2]),
      arg_r9: ptrString(args[3]),
      parent_before: snapshotFields(args[0], 0x500),
    };
    pushMapStack(registrationStackByThread, tid, rec);
    bump(counts, 'registration_entry_count');
  },
  onLeave(retval) {
    const rec = popMapStack(registrationStackByThread, tidKey());
    if (rec && rec.target_loader_seen) {
      logEvent('registration_leave_target_route', {
        is_target: true,
        parent_ptr: rec.parent_ptr,
        retval: ptrString(retval),
      }, this.context, true);
    }
  },
}));

function makePreCallHook(slotName, ownerOffset) {
  return () => ({
    onEnter() {
      const tid = tidKey();
      const reg = peekMapStack(registrationStackByThread, tid);
      const parent = this.context.rbx;
      const owner = parent.add(ownerOffset);
      const call = {
        slot: slotName,
        parent_ptr: ptrString(parent),
        owner_ptr: ptrString(owner),
        external_arg_ptr: ptrString(this.context.rdi),
        aux_ptr: ptrString(this.context.rbp),
        mode: this.context.rsi.toInt32(),
        parent_before: reg ? reg.parent_before : snapshotFields(parent, 0x500),
        owner_before: snapshotFields(owner, ownerOffset === 0x100 ? 0x220 : 0x220),
      };
      callerPreByThread[tid] = call;
      logEvent('registration_pre_loader_call', {
        slot: slotName,
        parent_ptr: call.parent_ptr,
        owner_ptr: call.owner_ptr,
        external_arg_ptr: call.external_arg_ptr,
      }, this.context, false);
    },
  });
}

installRvaHook('registration_first_pre_call', RVAS.registration_first_pre_call, makePreCallHook('first_parent_plus_0x100', 0x100));
installRvaHook('registration_second_pre_call', RVAS.registration_second_pre_call, makePreCallHook('second_parent_plus_0x250', 0x250));

function makePostCallHook(slotName) {
  return () => ({
    onEnter() {
      const tid = tidKey();
      const rec = lastLoaderByThread[tid];
      if (!rec) return;
      rec.return_eax = this.context.rax.toInt32();
      rec.post_call_site = slotName;
      rec.parent_at_post_call = ptrString(this.context.rbx);
      if (rec.return_eax !== 0 && isTargetRecord(rec)) {
        const reg = peekMapStack(registrationStackByThread, tid);
        if (reg) reg.target_loader_seen = true;
        emitTargetRecord('loader_caller_post_return', rec, this.context, {
          site: slotName,
          eax: rec.return_eax,
          parent_ptr: rec.parent_at_post_call,
        });
      }
    },
  });
}
installRvaHook('registration_first_post_call', RVAS.registration_first_post_call, makePostCallHook('first_post_0x3a3e1ac'));
installRvaHook('registration_second_post_call', RVAS.registration_second_post_call, makePostCallHook('second_post_0x3a3e1d6'));

installRvaHook('registration_first_success_store', RVAS.registration_first_success_store, () => ({
  onEnter() {
    const rec = lastLoaderByThread[tidKey()];
    if (rec && isTargetRecord(rec)) {
      rec.success_store_site = '0x3a3e1b0';
      rec.parent_before_success_store = snapshotFields(this.context.rbx, 0x500);
      emitTargetRecord('registration_success_store_before', rec, this.context, {
        parent_ptr: ptrString(this.context.rbx),
        store: '[rbx+0x3f4] = 1',
      });
    }
  },
}));

installRvaHook('registration_success_done', RVAS.registration_success_done, () => ({
  onEnter() {
    const rec = lastLoaderByThread[tidKey()];
    if (!rec || !isTargetRecord(rec)) return;
    const parent = this.context.rbx;
    rec.parent_after_success = snapshotFields(parent, 0x500);
    rec.owner_after_success = rec.owner_ptr ? snapshotFields(ptr(rec.owner_ptr), 0x220) : null;
    rec.parent_diff = diffSnapshots(rec.parent_before, rec.parent_after_success, rec.dest_ptr, rec.body_size);
    rec.owner_diff = diffSnapshots(rec.owner_before, rec.owner_after_success, rec.dest_ptr, rec.body_size);
    rec.fields_equal_dest_ptr = {
      parent: rec.parent_diff ? rec.parent_diff.fields_equal_dest_ptr : [],
      owner: rec.owner_diff ? rec.owner_diff.fields_equal_dest_ptr : [],
      r13_owner: rec.r13_after_diff ? rec.r13_after_diff.fields_equal_dest_ptr : [],
    };
    rec.fields_equal_body_size = {
      parent: rec.parent_diff ? rec.parent_diff.fields_equal_body_size : [],
      owner: rec.owner_diff ? rec.owner_diff.fields_equal_body_size : [],
      r13_owner: rec.r13_after_diff ? rec.r13_after_diff.fields_equal_body_size : [],
    };
    rec.nested_fields_containing_dest = {
      parent: fieldsContainingDest(rec.parent_after_success, rec.dest_ptr),
      owner: fieldsContainingDest(rec.owner_after_success, rec.dest_ptr),
    };
    emitTargetRecord('registration_success_path_done', rec, this.context, {
      parent_diff: rec.parent_diff,
      owner_diff: rec.owner_diff,
    });
  },
}));

installRvaHook('registration_fallback_parser', RVAS.registration_fallback_parser, () => ({
  onEnter(args) {
    logEvent('registration_fallback_parser_call', {
      caller_rva: rvaOf(this.returnAddress),
      rcx: ptrString(args[0]),
      rdx: ptrString(args[1]),
    }, this.context, false);
  },
}));
installRvaHook('registration_fallback_cleanup', RVAS.registration_fallback_cleanup, () => ({
  onEnter(args) {
    logEvent('registration_fallback_cleanup_call', {
      caller_rva: rvaOf(this.returnAddress),
      rcx: ptrString(args[0]),
    }, this.context, false);
  },
}));

installRvaHook('loader_entry', RVAS.loader_entry, () => ({
  onEnter(args) {
    const tid = tidKey();
    const call = callerPreByThread[tid] || {};
    const rec = {
      invocation_index: loaderInvocationIndex++,
      caller_rva: rvaOf(this.returnAddress),
      entry_args: [ptrString(args[0]), ptrString(args[1]), ptrString(args[2]), ptrString(args[3])],
      slot: call.slot || null,
      owner_ptr: ptrString(args[0]),
      parent_ptr: call.parent_ptr || null,
      external_arg_ptr: ptrString(args[1]),
      owner_before: call.owner_before || snapshotFields(args[0], 0x220),
      parent_before: call.parent_before || null,
      r13_entry: ptrString(this.context.r13),
      r15_entry: ptrString(this.context.r15),
    };
    pushMapStack(loaderStackByThread, tid, rec);
    bump(counts, 'loader_entry_count');
  },
  onLeave(retval) {
    const tid = tidKey();
    const rec = popMapStack(loaderStackByThread, tid);
    if (!rec) return;
    rec.loader_return_rax = ptrString(retval);
    rec.loader_return_eax = retval.toInt32();
    rec.owner_after_loader = rec.owner_ptr ? snapshotFields(ptr(rec.owner_ptr), 0x220) : null;
    rec.r13_after_loader = rec.r13_ptr ? snapshotFields(ptr(rec.r13_ptr), 0x200) : null;
    rec.r15_after_loader = rec.r15_ptr ? snapshotFields(ptr(rec.r15_ptr), 0x200) : null;
    rec.owner_loader_diff = diffSnapshots(rec.owner_before, rec.owner_after_loader, rec.dest_ptr, rec.body_size);
    rec.r13_after_diff = diffSnapshots(rec.r13_before_alloc, rec.r13_after_loader, rec.dest_ptr, rec.body_size);
    lastLoaderByThread[tid] = rec;
    if (isTargetRecord(rec)) {
      targetRecords.push(summarizeRecord(rec));
      emitTargetRecord('loader_leave_target', rec, this.context, {
        owner_loader_diff: rec.owner_loader_diff,
        r13_after_diff: rec.r13_after_diff,
      });
    }
  },
}));

installRvaHook('loader_id_check_post', RVAS.loader_id_check_post, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    const idPtr = this.context.rsp.add(0x3d0);
    const idAscii = safeAsciiFixed(idPtr, 0x28);
    rec.external_id_ascii = idAscii;
    rec.external_id_stack_ptr = ptrString(idPtr);
    rec.external_id_compare_result_eax = this.context.rax.toInt32();
    rec.compare_expected_ptr_r12 = ptrString(this.context.r12);
    rec.compare_expected_ascii = safeAsciiFixed(this.context.r12, 0x28);
    if (idAscii === TARGET_ID || rec.compare_expected_ascii === TARGET_ID) {
      emitTargetRecord('loader_target_id_compare', rec, this.context, {
        external_id_ascii: idAscii,
        compare_expected_ascii: rec.compare_expected_ascii,
        eax: rec.external_id_compare_result_eax,
      });
    }
  },
}));

installRvaHook('loader_post_body_size', RVAS.loader_post_body_size, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.body_size = this.context.rax.toInt32();
    rec.stream_ptr = ptrString(this.context.rcx);
    if (rec.body_size === TARGET_BODY_SIZE || rec.external_id_ascii === TARGET_ID) {
      emitTargetRecord('loader_body_size', rec, this.context, { body_size: rec.body_size });
    }
  },
}));

installRvaHook('loader_post_allocation', RVAS.loader_post_allocation, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.dest_ptr = ptrString(this.context.rax);
    rec.requested_size = this.context.rdi.toInt32();
    rec.r13_ptr = ptrString(this.context.r13);
    rec.r15_ptr = ptrString(this.context.r15);
    rec.r13_before_alloc = rec.r13_ptr ? snapshotFields(ptr(rec.r13_ptr), 0x200) : null;
    rec.dest_prefix_before_read_hex = hexPrefix(this.context.rax, Math.min(rec.requested_size || 0, 32));
    if (rec.body_size === TARGET_BODY_SIZE || rec.external_id_ascii === TARGET_ID) {
      emitTargetRecord('loader_allocation_result', rec, this.context, {
        dest_ptr: rec.dest_ptr,
        requested_size: rec.requested_size,
        r13_ptr: rec.r13_ptr,
      });
    }
  },
}));

installRvaHook('loader_post_body_read', RVAS.loader_post_body_read, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    const dest = rec.dest_ptr ? ptr(rec.dest_ptr) : ptr(0);
    const n = rec.body_size || rec.requested_size || 0;
    rec.returned_read_size = this.context.rax.toInt32();
    rec.dest_hash = fnv1a32(dest, n);
    rec.body_prefix_hex = hexPrefix(dest, Math.min(n, 96));
    rec.body_prefix_ascii = safeAsciiFixed(dest, Math.min(n, 96));
    rec.owner_after_body_read = rec.owner_ptr ? snapshotFields(ptr(rec.owner_ptr), 0x220) : null;
    if (isTargetRecord(rec)) {
      emitTargetRecord('loader_body_read_result', rec, this.context, {
        returned_read_size: rec.returned_read_size,
        dest_ptr: rec.dest_ptr,
        dest_hash: rec.dest_hash,
      });
    }
  },
}));

function writeSummary(reason) {
  writeJson({
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    target_counts: targetCounts,
    caller_counts: callerCounts,
    total_loader_invocations: loaderInvocationIndex,
    total_target_records: targetRecords.length,
    target_records: targetRecords.slice(-10),
    active_loader_stacks: Object.fromEntries(Object.entries(loaderStackByThread).map(([k, v]) => [k, v.length])),
    active_registration_stacks: Object.fromEntries(Object.entries(registrationStackByThread).map(([k, v]) => [k, v.length])),
  });
}

logEvent('ready', {
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  target_id: TARGET_ID,
  target_body_size: TARGET_BODY_SIZE,
  target_hash: TARGET_HASH,
  hook_rvas: Object.fromEntries(Object.entries(RVAS).map(([k, v]) => [k, `0x${v.toString(16)}`])),
}, null, false);

const summaryTimer = setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => {
  clearInterval(summaryTimer);
  writeSummary('unload');
  out.close();
});
