// Read-only target body ownership trace.
//
// Finds where the target VectorData body dest_ptr is owned after
// 0x143A41780 reads the 2644-byte CHNKExta body.
//
// No renderer hooks. No row-span hooks. No memory patching.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_BODY_SIZE = 2644;
const TARGET_HASH = 'fnv1a32:7bece4ac';
const SNAPSHOT_SIZE = 0x300;
const NESTED_SCAN_SIZE = 0x100;
const MAX_POINTER_WRITES = 100;
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  loader_entry: 0x3a41780,
  id_check_post: 0x3a41d48,
  body_size_call: 0x3a41d53,
  body_size_post: 0x3a41d58,
  allocation_call: 0x3a41d68,
  allocation_post: 0x3a41d6d,
  body_read_call: 0x3a41d7a,
  body_read_post: 0x3a41d7f,
  reserve_entry: 0x2056880,
  reader_entry: 0x20575a0,
  cleanup_entry: 0x20493f0,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_target_body_ownership_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let invocationSeq = 0;
let reserveSeq = 0;
let readerSeq = 0;
let backtracesWritten = 0;
const counts = {};
const targetRecords = [];
const loaderStackByThread = {};
const reserveStackByThread = {};
const readerStackByThread = {};
const lastReserveByThread = {};
const lastReaderByThread = {};
let writeMonitorActive = false;

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}
function tidKey() { return String(Process.getCurrentThreadId()); }
function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }
function ptrString(p) {
  try { return (!p || p.isNull()) ? null : p.toString(); } catch (_) { return null; }
}
function rvaOf(p) {
  try {
    if (!p || p.isNull()) return null;
    const d = p.sub(cspBase);
    if (d.compare(ptr(0)) < 0 || d.compare(ptr(0x8000000)) > 0) return null;
    return `0x${d.toUInt32().toString(16)}`;
  } catch (_) { return null; }
}
function bump(name) { counts[name] = (counts[name] || 0) + 1; }
function pushStack(map, key, value) {
  if (!map[key]) map[key] = [];
  map[key].push(value);
}
function popStack(map, key) {
  const stack = map[key] || [];
  if (!stack.length) return null;
  const value = stack.pop();
  if (!stack.length) delete map[key];
  return value;
}
function peekStack(map, key) {
  const stack = map[key] || [];
  return stack.length ? stack[stack.length - 1] : null;
}
function currentLoader() { return peekStack(loaderStackByThread, tidKey()); }
function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}
function logEvent(event, details, context, wantBacktrace) {
  bump(event);
  const row = {
    event,
    timestamp_ms: nowMs(),
    event_index: eventIndex++,
    process_id: processId,
    thread_id: Process.getCurrentThreadId(),
    ...details,
  };
  if (wantBacktrace && context && backtracesWritten < 80) {
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
  let outStr = '';
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i];
    outStr += (b >= 0x20 && b <= 0x7e) ? String.fromCharCode(b) : '.';
  }
  return outStr;
}
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
function safeReadPointer(p) {
  try { return p.readPointer(); } catch (_) { return null; }
}
function safeReadU32(p) {
  try { return p.readU32(); } catch (_) { return null; }
}
function safeReadU64String(p) {
  try { return p.readU64().toString(); } catch (_) { return null; }
}

function snapshotObject(label, basePtr, destPtr, bodySize) {
  if (!basePtr || basePtr.isNull()) return null;
  const fields = [];
  for (let off = 0; off < SNAPSHOT_SIZE; off += 8) {
    const addr = basePtr.add(off);
    const p = safeReadPointer(addr);
    const ptrValue = ptrString(p);
    const u32Value = safeReadU32(addr);
    const u64Value = safeReadU64String(addr);
    const row = {
      offset: off,
      offset_hex: `0x${off.toString(16)}`,
      address: ptrString(addr),
      ptr: ptrValue,
      u32: u32Value,
      u64: u64Value,
      equals_dest_ptr: Boolean(destPtr && ptrValue === destPtr),
      equals_body_size: bodySize !== null && bodySize !== undefined && u32Value === bodySize,
      contains_dest_ptr: false,
      nested_offsets: [],
    };
    if (destPtr && ptrValue && ptrValue !== '0x0') {
      try {
        const nested = ptr(ptrValue);
        for (let noff = 0; noff < NESTED_SCAN_SIZE; noff += 8) {
          const np = safeReadPointer(nested.add(noff));
          if (ptrString(np) === destPtr) {
            row.contains_dest_ptr = true;
            row.nested_offsets.push(`0x${noff.toString(16)}`);
          }
        }
      } catch (_) {}
    }
    fields.push(row);
  }
  return { label, base: ptrString(basePtr), size: SNAPSHOT_SIZE, fields };
}

function diffSnapshots(before, after) {
  if (!before || !after) return null;
  const beforeByOff = {};
  for (const row of before.fields || []) beforeByOff[row.offset] = row;
  const changed = [];
  const destFields = [];
  const bodySizeFields = [];
  const nestedDestFields = [];
  for (const row of after.fields || []) {
    const old = beforeByOff[row.offset] || {};
    if (old.ptr !== row.ptr || old.u32 !== row.u32 || old.u64 !== row.u64) {
      changed.push({
        offset_hex: row.offset_hex,
        before_ptr: old.ptr,
        after_ptr: row.ptr,
        before_u32: old.u32,
        after_u32: row.u32,
        before_u64: old.u64,
        after_u64: row.u64,
      });
    }
    if (row.equals_dest_ptr) destFields.push(row.offset_hex);
    if (row.equals_body_size) bodySizeFields.push(row.offset_hex);
    if (row.contains_dest_ptr) {
      nestedDestFields.push({
        offset_hex: row.offset_hex,
        ptr: row.ptr,
        nested_offsets: row.nested_offsets,
      });
    }
  }
  return { changed, dest_ptr_fields: destFields, body_size_fields: bodySizeFields, nested_dest_ptr_fields: nestedDestFields };
}

function isTarget(rec) {
  return Boolean(rec && (
    rec.external_id_ascii === TARGET_ID ||
    rec.body_size === TARGET_BODY_SIZE ||
    rec.dest_hash === TARGET_HASH
  ));
}

function captureNamedSnapshots(rec, phase) {
  if (!rec) return {};
  const dest = rec.dest_ptr || null;
  const size = rec.body_size;
  const pairs = {
    entry_rcx: rec.entry_rcx,
    entry_rdx: rec.entry_rdx,
    rbx: rec.rbx,
    r13: rec.r13,
    r15: rec.r15,
    reserve_owner: rec.reserve_owner_ptr,
    reserve_return_owner: rec.reserve_return_owner_ptr,
    body_read_stream: rec.body_read_stream_ptr,
    body_read_dest_arg: rec.body_read_dest_ptr,
  };
  const outRows = {};
  for (const [label, value] of Object.entries(pairs)) {
    if (!value) continue;
    try { outRows[label] = snapshotObject(`${phase}:${label}`, ptr(value), dest, size); } catch (_) {}
  }
  return outRows;
}

function diffSnapshotMaps(before, after) {
  const outRows = {};
  for (const key of Object.keys(after || {})) {
    outRows[key] = diffSnapshots(before ? before[key] : null, after[key]);
  }
  return outRows;
}

function collectOwnership(rec, snapshots) {
  const owners = [];
  for (const [label, snap] of Object.entries(snapshots || {})) {
    if (!snap) continue;
    for (const field of snap.fields || []) {
      if (field.equals_dest_ptr || field.equals_body_size || field.contains_dest_ptr) {
        owners.push({
          object_label: label,
          object_ptr: snap.base,
          field_offset: field.offset_hex,
          field_ptr: field.ptr,
          contains_dest_ptr: field.contains_dest_ptr,
          nested_offsets: field.nested_offsets,
          equals_dest_ptr: field.equals_dest_ptr,
          equals_body_size: field.equals_body_size,
        });
      }
    }
  }
  return owners;
}

function emitTargetRecord(event, rec, context, extra) {
  if (!isTarget(rec)) return;
  const row = {
    invocation_id: rec.invocation_id,
    external_id_ascii: rec.external_id_ascii,
    body_size: rec.body_size,
    dest_ptr: rec.dest_ptr,
    dest_hash: rec.dest_hash,
    body_prefix_hex: rec.body_prefix_hex,
    body_prefix_ascii: rec.body_prefix_ascii,
    return_value: rec.return_value,
    registers: {
      entry_rcx: rec.entry_rcx,
      entry_rdx: rec.entry_rdx,
      entry_r8: rec.entry_r8,
      entry_r9: rec.entry_r9,
      rbx: rec.rbx,
      r12: rec.r12,
      r13: rec.r13,
      r14: rec.r14,
      r15: rec.r15,
    },
    parent_object_pointer_candidates: [rec.entry_rcx, rec.rbx].filter(Boolean),
    allocator_owner_pointer_candidates: [rec.entry_rdx, rec.r13, rec.reserve_owner_ptr, rec.reserve_return_owner_ptr].filter(Boolean),
    ownership_candidates: rec.ownership_candidates || [],
    ...extra,
  };
  logEvent(event, { is_target: true, target_id: TARGET_ID, record: row }, context, true);
}

function install(name, rva, callbacks) {
  try {
    const va = vaOfRva(rva);
    Interceptor.attach(va, callbacks(va));
    logEvent('hook_installed', { name, rva: `0x${rva.toString(16)}`, va: va.toString() }, null, false);
  } catch (e) {
    logEvent('hook_install_failed', { name, rva: `0x${rva.toString(16)}`, error: String(e) }, null, false);
  }
}

function maybeEnableWriteDetector(rec) {
  if (writeMonitorActive || !rec || !rec.dest_ptr || typeof MemoryAccessMonitor === 'undefined') return;
  const ranges = [];
  const seen = {};
  const addRange = (label, pstr) => {
    if (!pstr || pstr === '0x0' || seen[pstr]) return;
    try {
      const base = ptr(pstr);
      ranges.push({ base, size: SNAPSHOT_SIZE, label });
      seen[pstr] = true;
    } catch (_) {}
  };
  addRange('entry_rcx', rec.entry_rcx);
  addRange('entry_rdx', rec.entry_rdx);
  addRange('rbx', rec.rbx);
  addRange('r13', rec.r13);
  addRange('r15', rec.r15);
  addRange('reserve_owner', rec.reserve_owner_ptr);
  addRange('reserve_return_owner', rec.reserve_return_owner_ptr);
  if (!ranges.length) return;
  rec.pointer_value_writes = rec.pointer_value_writes || [];
  try {
    MemoryAccessMonitor.enable(ranges.map((r) => ({ base: r.base, size: r.size })), {
      onAccess(details) {
        if (details.operation !== 'write') return;
        if (!currentLoader() || currentLoader().invocation_id !== rec.invocation_id) return;
        if (rec.pointer_value_writes.length >= MAX_POINTER_WRITES) return;
        const addr = details.address;
        let written = null;
        try { written = ptrString(addr.readPointer()); } catch (_) {}
        if (written !== rec.dest_ptr) return;
        const owner = ranges.find((r) => addr.compare(r.base) >= 0 && addr.compare(r.base.add(r.size)) < 0);
        const row = {
          pc: ptrString(details.from),
          pc_rva: rvaOf(details.from),
          destination_address: ptrString(addr),
          written_value: written,
          base_object_candidate: owner ? owner.label : null,
          base_object_ptr: owner ? ptrString(owner.base) : null,
          offset_from_base: owner ? addr.sub(owner.base).toString() : null,
        };
        rec.pointer_value_writes.push(row);
        logEvent('pointer_value_write_dest_ptr', {
          is_target: true,
          invocation_id: rec.invocation_id,
          write: row,
        }, null, false);
      },
    });
    writeMonitorActive = true;
    logEvent('pointer_write_detector_enabled', {
      is_target: true,
      invocation_id: rec.invocation_id,
      ranges: ranges.map((r) => ({ label: r.label, base: ptrString(r.base), size: r.size })),
    }, null, false);
  } catch (e) {
    logEvent('pointer_write_detector_unavailable', {
      is_target: true,
      invocation_id: rec.invocation_id,
      error: String(e),
    }, null, false);
  }
}

function disableWriteDetector(reason) {
  if (!writeMonitorActive) return;
  try { MemoryAccessMonitor.disable(); } catch (_) {}
  writeMonitorActive = false;
  logEvent('pointer_write_detector_disabled', { reason }, null, false);
}

install('loader_entry_0x143A41780', RVAS.loader_entry, () => ({
  onEnter(args) {
    const rec = {
      invocation_id: invocationSeq++,
      caller_rva: rvaOf(this.returnAddress),
      entry_rcx: ptrString(args[0]),
      entry_rdx: ptrString(args[1]),
      entry_r8: ptrString(args[2]),
      entry_r9: ptrString(args[3]),
      entry_r9d: args[3].toInt32(),
      rbx: ptrString(this.context.rbx),
      r12: ptrString(this.context.r12),
      r13: ptrString(this.context.r13),
      r14: ptrString(this.context.r14),
      r15: ptrString(this.context.r15),
      snapshots_entry: {
        entry_rcx: snapshotObject('entry:entry_rcx', args[0], null, null),
        entry_rdx: snapshotObject('entry:entry_rdx', args[1], null, null),
      },
      cleanup_calls: [],
      post_body_read_calls: [],
      pointer_value_writes: [],
    };
    pushStack(loaderStackByThread, tidKey(), rec);
  },
  onLeave(retval) {
    const rec = popStack(loaderStackByThread, tidKey());
    if (!rec) return;
    rec.return_value = ptrString(retval);
    rec.return_eax = retval.toInt32();
    rec.snapshots_before_return = captureNamedSnapshots(rec, 'before_return');
    rec.before_return_diffs = diffSnapshotMaps(rec.snapshots_entry_plus, rec.snapshots_before_return);
    rec.ownership_candidates = collectOwnership(rec, rec.snapshots_before_return);
    if (isTarget(rec)) {
      targetRecords.push({
        invocation_id: rec.invocation_id,
        external_id_ascii: rec.external_id_ascii,
        body_size: rec.body_size,
        dest_ptr: rec.dest_ptr,
        dest_hash: rec.dest_hash,
        ownership_candidates: rec.ownership_candidates,
      });
      emitTargetRecord('loader_leave_target_ownership', rec, this.context, {
        snapshots_before_return: rec.snapshots_before_return,
        diffs_before_return: rec.before_return_diffs,
        cleanup_calls: rec.cleanup_calls,
        pointer_value_writes: rec.pointer_value_writes,
      });
    }
    disableWriteDetector('loader_leave');
  },
}));

install('id_check_post_0x3A41D48', RVAS.id_check_post, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    const idPtr = this.context.rsp.add(0x3d0);
    rec.external_id_ascii = safeAsciiFixed(idPtr, 0x28);
    rec.id_compare_eax = this.context.rax.toInt32();
    rec.rbx = ptrString(this.context.rbx);
    rec.r12 = ptrString(this.context.r12);
    rec.r13 = ptrString(this.context.r13);
    rec.r14 = ptrString(this.context.r14);
    rec.r15 = ptrString(this.context.r15);
    if (rec.external_id_ascii === TARGET_ID) {
      emitTargetRecord('target_external_id_check', rec, this.context, {
        external_id_ascii: rec.external_id_ascii,
        id_compare_eax: rec.id_compare_eax,
      });
    }
  },
}));

install('body_size_call_0x3A41D53', RVAS.body_size_call, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.body_size_stream_ptr = ptrString(this.context.rcx);
  },
}));
install('body_size_post_0x3A41D58', RVAS.body_size_post, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.body_size = this.context.rax.toInt32();
    if (isTarget(rec)) emitTargetRecord('target_body_size_read', rec, this.context, { body_size: rec.body_size });
  },
}));

install('reserve_entry_0x142056880', RVAS.reserve_entry, () => ({
  onEnter(args) {
    const rec = {
      reserve_id: reserveSeq++,
      caller_rva: rvaOf(this.returnAddress),
      owner_ptr: ptrString(args[0]),
      requested_size: args[1].toInt32(),
      entry_snapshots: {
        owner: snapshotObject('reserve_entry:owner', args[0], null, args[1].toInt32()),
      },
    };
    pushStack(reserveStackByThread, tidKey(), rec);
  },
  onLeave(retval) {
    const rec = popStack(reserveStackByThread, tidKey());
    if (!rec) return;
    rec.return_ptr = ptrString(retval);
    rec.leave_snapshots = {
      owner: rec.owner_ptr ? snapshotObject('reserve_leave:owner', ptr(rec.owner_ptr), rec.return_ptr, rec.requested_size) : null,
    };
    rec.diff = diffSnapshotMaps(rec.entry_snapshots, rec.leave_snapshots);
    lastReserveByThread[tidKey()] = rec;
    const loader = currentLoader();
    if (loader && (rec.caller_rva === '0x3a41d6d' || loader.body_size === TARGET_BODY_SIZE || loader.external_id_ascii === TARGET_ID)) {
      loader.reserve_call = rec;
      loader.reserve_owner_ptr = rec.owner_ptr;
      loader.reserve_return_owner_ptr = rec.owner_ptr;
      loader.reserve_return_ptr = rec.return_ptr;
      if (!loader.dest_ptr) loader.dest_ptr = rec.return_ptr;
      if (isTarget(loader)) {
        logEvent('target_reserve_leave', {
          is_target: true,
          invocation_id: loader.invocation_id,
          reserve: rec,
        }, this.context, true);
      }
    }
  },
}));

install('allocation_call_0x3A41D68', RVAS.allocation_call, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.reserve_owner_ptr = ptrString(this.context.r13);
    rec.reserve_requested_size = this.context.rdi.toInt32();
    rec.snapshots_before_allocation = captureNamedSnapshots(rec, 'before_allocation');
  },
}));
install('allocation_post_0x3A41D6D', RVAS.allocation_post, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.dest_ptr = ptrString(this.context.rax);
    rec.reserve_return_ptr = rec.dest_ptr;
    rec.reserve_owner_ptr = ptrString(this.context.r13);
    rec.rbx = ptrString(this.context.rbx);
    rec.r12 = ptrString(this.context.r12);
    rec.r13 = ptrString(this.context.r13);
    rec.r14 = ptrString(this.context.r14);
    rec.r15 = ptrString(this.context.r15);
    rec.snapshots_after_allocation = captureNamedSnapshots(rec, 'after_allocation');
    rec.post_allocation_changes = diffSnapshotMaps(rec.snapshots_before_allocation, rec.snapshots_after_allocation);
    rec.ownership_after_allocation = collectOwnership(rec, rec.snapshots_after_allocation);
    if (isTarget(rec)) {
      maybeEnableWriteDetector(rec);
      emitTargetRecord('target_allocation_result', rec, this.context, {
        post_allocation_changes: rec.post_allocation_changes,
        ownership_after_allocation: rec.ownership_after_allocation,
      });
    }
  },
}));

install('reader_entry_0x1420575A0', RVAS.reader_entry, () => ({
  onEnter(args) {
    const rec = {
      reader_id: readerSeq++,
      caller_rva: rvaOf(this.returnAddress),
      stream_ptr: ptrString(args[0]),
      dest_arg: ptrString(args[1]),
      requested_size: args[2].toInt32(),
      dest_prefix_before: hexPrefix(args[1], Math.min(args[2].toInt32(), 32)),
    };
    pushStack(readerStackByThread, tidKey(), rec);
  },
  onLeave(retval) {
    const rec = popStack(readerStackByThread, tidKey());
    if (!rec) return;
    rec.return_value = ptrString(retval);
    rec.return_i32 = retval.toInt32();
    rec.dest_hash_after = rec.dest_arg ? fnv1a32(ptr(rec.dest_arg), rec.requested_size) : null;
    rec.dest_prefix_after = rec.dest_arg ? hexPrefix(ptr(rec.dest_arg), Math.min(rec.requested_size, 96)) : null;
    lastReaderByThread[tidKey()] = rec;
    const loader = currentLoader();
    if (loader && (rec.caller_rva === '0x3a41d7f' || rec.requested_size === TARGET_BODY_SIZE)) {
      loader.body_reader_call = rec;
      loader.body_read_stream_ptr = rec.stream_ptr;
      loader.body_read_dest_ptr = rec.dest_arg;
      if (!loader.dest_ptr) loader.dest_ptr = rec.dest_arg;
      loader.dest_hash = rec.dest_hash_after;
      loader.body_prefix_hex = rec.dest_prefix_after;
      loader.body_prefix_ascii = rec.dest_arg ? safeAsciiFixed(ptr(rec.dest_arg), Math.min(rec.requested_size, 96)) : null;
      if (isTarget(loader)) {
        logEvent('target_body_reader_leave', {
          is_target: true,
          invocation_id: loader.invocation_id,
          reader: rec,
        }, this.context, true);
      }
    }
  },
}));

install('body_read_call_0x3A41D7A', RVAS.body_read_call, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.body_read_stream_ptr = ptrString(this.context.rcx);
    rec.body_read_dest_ptr = ptrString(this.context.rdx);
    rec.body_read_size_arg = this.context.r8.toInt32();
    rec.snapshots_before_body_read = captureNamedSnapshots(rec, 'before_body_read');
  },
}));
install('body_read_post_0x3A41D7F', RVAS.body_read_post, () => ({
  onEnter() {
    const rec = currentLoader();
    if (!rec) return;
    rec.body_read_return_value = ptrString(this.context.rax);
    if (!rec.dest_ptr) rec.dest_ptr = rec.body_read_dest_ptr;
    const dest = rec.dest_ptr ? ptr(rec.dest_ptr) : ptr(0);
    const n = rec.body_size || rec.body_read_size_arg || 0;
    rec.dest_hash = fnv1a32(dest, n);
    rec.body_prefix_hex = hexPrefix(dest, Math.min(n, 96));
    rec.body_prefix_ascii = safeAsciiFixed(dest, Math.min(n, 96));
    rec.snapshots_after_body_read = captureNamedSnapshots(rec, 'after_body_read');
    rec.post_read_changes = diffSnapshotMaps(rec.snapshots_before_body_read, rec.snapshots_after_body_read);
    rec.ownership_after_body_read = collectOwnership(rec, rec.snapshots_after_body_read);
    if (isTarget(rec)) {
      emitTargetRecord('target_body_read_post', rec, this.context, {
        body_read_return_value: rec.body_read_return_value,
        post_read_changes: rec.post_read_changes,
        ownership_after_body_read: rec.ownership_after_body_read,
      });
    }
  },
}));

install('cleanup_entry_0x1420493F0', RVAS.cleanup_entry, () => ({
  onEnter(args) {
    const rec = currentLoader();
    if (!rec || !isTarget(rec)) return;
    const call = {
      caller_rva: rvaOf(this.returnAddress),
      rcx: ptrString(args[0]),
      snapshot_before_cleanup: snapshotObject('cleanup_arg', args[0], rec.dest_ptr, rec.body_size),
    };
    rec.cleanup_calls.push(call);
    logEvent('target_cleanup_call', {
      is_target: true,
      invocation_id: rec.invocation_id,
      cleanup: call,
    }, this.context, true);
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
    total_target_records: targetRecords.length,
    target_records: targetRecords.slice(-10),
    active_loader_stacks: Object.fromEntries(Object.entries(loaderStackByThread).map(([k, v]) => [k, v.length])),
    active_reserve_stacks: Object.fromEntries(Object.entries(reserveStackByThread).map(([k, v]) => [k, v.length])),
    active_reader_stacks: Object.fromEntries(Object.entries(readerStackByThread).map(([k, v]) => [k, v.length])),
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
  disableWriteDetector('unload');
  writeSummary('unload');
  out.close();
});
