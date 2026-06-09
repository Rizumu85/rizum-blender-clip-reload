// Read-only registration-route trace for 0x142049220 and its helpers.
//
// This probes whether target metadata descriptors are inserted into a registry
// inside 0x142049220, or whether the function only initializes descriptor-owned
// fields/string storage.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const SNAPSHOT_SIZE = 0x100;
const SUMMARY_INTERVAL_MS = 5000;
const MAX_BACKTRACES = 80;
const MAX_OTHER_SAMPLES = 40;

const RVAS = {
  wrapper_142049220: 0x2049220,
  helper_142049920: 0x2049920,
  helper_14204A290: 0x204a290,
};

const STUBS = {
  VectorData: { stub: 0x0cdf60, return_site: 0x0cdf77, descriptor: 0x5486110 },
  VectorObjectList: { stub: 0x139830, return_site: 0x139847, descriptor: 0x54f2be8 },
  TimeLapseBlob: { stub: 0x1396b0, return_site: 0x1396c7, descriptor: 0x54f3788 },
};

const TARGET_STRINGS = ['VectorData', 'VectorObjectList', 'TimeLapseBlob', 'ExternalChunk'];
const TARGET_RETURN_SITES = new Map(Object.entries(STUBS).map(([name, item]) => [`0x${item.return_site.toString(16)}`, name]));

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_descriptor_registry_registration_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let wrapperIndex = 0;
let helperIndex = 0;
let backtracesWritten = 0;
let otherSamples = 0;
const activeWrappersByThread = new Map();
const counts = {};
const targetStringCounts = {};
const helperCounts = {};
const candidateRegistryPointers = {};
const descriptors = {};
const firstTargetEvents = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}
function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }
function tidKey() { return String(Process.getCurrentThreadId()); }
function ptrString(p) { try { return !p || p.isNull() ? null : p.toString(); } catch (_) { return null; } }
function rvaOf(p) {
  try {
    if (!p || p.isNull()) return null;
    const delta = p.sub(cspBase);
    if (delta.compare(ptr(0)) < 0 || delta.compare(ptr(0x8000000)) > 0) return null;
    return `0x${delta.toUInt32().toString(16)}`;
  } catch (_) { return null; }
}
function bump(map, key) { const k = key ?? 'unknown'; map[k] = (map[k] || 0) + 1; }
function writeJson(row) { out.write(JSON.stringify(row) + '\n'); out.flush(); }

function cleanText(text) {
  if (typeof text !== 'string' || text.length === 0 || text.length > 512) return null;
  let printable = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if ((c >= 0x20 && c <= 0x7e) || c === 9 || c === 10 || c === 13) printable++;
  }
  return printable >= Math.min(text.length, 4) ? text : null;
}
function readUtf8(p, maxLen) {
  try { if (!p || p.isNull()) return null; return cleanText(p.readUtf8String(maxLen || 256)); }
  catch (_) { try { return cleanText(p.readCString(maxLen || 256)); } catch (_2) { return null; } }
}
function readUtf16(p, maxChars) {
  try { if (!p || p.isNull()) return null; return cleanText(p.readUtf16String(maxChars || 128)); }
  catch (_) { return null; }
}
function safeReadPointer(p) { try { if (!p || p.isNull()) return null; return p.readPointer(); } catch (_) { return null; } }
function safeReadU32(p) { try { if (!p || p.isNull()) return null; return p.readU32(); } catch (_) { return null; } }
function safeReadU64(p) { try { if (!p || p.isNull()) return null; return p.readU64().toString(); } catch (_) { return null; } }
function safeBytes(p, size) {
  try {
    if (!p || p.isNull()) return null;
    const arr = new Uint8Array(p.readByteArray(size));
    return Array.from(arr).map((x) => x.toString(16).padStart(2, '0')).join('');
  } catch (_) { return null; }
}
function descriptorSnapshot(p) {
  if (!p || p.isNull()) return null;
  const fields = [];
  for (let off = 0; off < SNAPSHOT_SIZE; off += 8) {
    const slot = p.add(off);
    const pp = safeReadPointer(slot);
    fields.push({
      offset: `0x${off.toString(16)}`,
      u64: safeReadU64(slot),
      u32: safeReadU32(slot),
      ptr: ptrString(pp),
      ptr_rva: rvaOf(pp),
      ptr_utf8: readUtf8(pp, 96),
      ptr_utf16: readUtf16(pp, 64),
    });
  }
  return { ptr: ptrString(p), rva: rvaOf(p), bytes_hex: safeBytes(p, SNAPSHOT_SIZE), fields };
}
function diffSnapshots(before, after) {
  if (!before || !after || !before.bytes_hex || !after.bytes_hex) return null;
  const diffs = [];
  for (let i = 0; i < Math.min(before.bytes_hex.length, after.bytes_hex.length); i += 2) {
    const b = before.bytes_hex.slice(i, i + 2);
    const a = after.bytes_hex.slice(i, i + 2);
    if (b !== a) diffs.push({ offset: `0x${(i / 2).toString(16)}`, before: b, after: a });
  }
  return diffs;
}
function pointerPreview(p) {
  const p0 = safeReadPointer(p);
  return {
    ptr: ptrString(p),
    rva: rvaOf(p),
    utf8: readUtf8(p, 256),
    utf16: readUtf16(p, 128),
    p0: ptrString(p0),
    p0_rva: rvaOf(p0),
    p0_utf8: readUtf8(p0, 256),
    p0_utf16: readUtf16(p0, 128),
    u32_0: safeReadU32(p),
    u64_0: safeReadU64(p),
  };
}
function findTargetHits(obj) {
  const text = JSON.stringify(obj);
  return TARGET_STRINGS.filter((needle) => text.includes(needle));
}
function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE).slice(0, 24)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) { row.backtrace = []; }
}
function activeStack() {
  const key = tidKey();
  if (!activeWrappersByThread.has(key)) activeWrappersByThread.set(key, []);
  return activeWrappersByThread.get(key);
}
function currentWrapper() {
  const stack = activeWrappersByThread.get(tidKey());
  return stack && stack.length ? stack[stack.length - 1] : null;
}

function hookStub(name, meta) {
  const descPtr = vaOfRva(meta.descriptor);
  descriptors[name] = { descriptor_rva: `0x${meta.descriptor.toString(16)}`, descriptor_pointer: ptrString(descPtr) };
  Interceptor.attach(vaOfRva(meta.stub), {
    onEnter(args) {
      bump(counts, 'stub_entry');
      writeJson({
        event: 'stub_entry',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        stub_name: name,
        stub_rva: `0x${meta.stub.toString(16)}`,
        caller_rva: rvaOf(this.returnAddress),
        descriptor_ptr: ptrString(descPtr),
        descriptor_static_rva: `0x${meta.descriptor.toString(16)}`,
      });
    },
  });
}

function hookWrapper() {
  Interceptor.attach(vaOfRva(RVAS.wrapper_142049220), {
    onEnter(args) {
      this.index = wrapperIndex++;
      this.callerRva = rvaOf(this.returnAddress);
      this.stubName = TARGET_RETURN_SITES.get(this.callerRva) || null;
      this.rcx = this.context.rcx;
      this.rdx = this.context.rdx;
      this.argsPreview = {
        rcx: pointerPreview(this.context.rcx),
        rdx: pointerPreview(this.context.rdx),
        r8: pointerPreview(this.context.r8),
        r9: pointerPreview(this.context.r9),
      };
      this.stringHits = findTargetHits(this.argsPreview);
      this.targetHits = this.stubName ? [this.stubName] : this.stringHits;
      this.relevant = Boolean(this.stubName) || this.stringHits.length > 0;
      bump(counts, 'wrapper_entry');
      if (this.relevant) {
        this.before = descriptorSnapshot(this.rcx);
        const ctx = {
          wrapper_call_index: this.index,
          caller_rva: this.callerRva,
          stub_name_from_return_site: this.stubName,
          target_hits: this.targetHits,
          descriptor_ptr: ptrString(this.rcx),
          descriptor_static_rva: rvaOf(this.rcx),
          string_key: readUtf8(this.rdx, 256),
          helper_events: [],
        };
        activeStack().push(ctx);
        this.activeContext = ctx;
      }
    },
    onLeave(retval) {
      if (!this.relevant) {
        if (otherSamples < MAX_OTHER_SAMPLES) {
          otherSamples++;
          writeJson({
            event: 'wrapper_other_sample',
            timestamp_ms: nowMs(),
            event_index: eventIndex++,
            process_id: processId,
            thread_id: Process.getCurrentThreadId(),
            wrapper_call_index: this.index,
            caller_rva: this.callerRva,
            rcx: ptrString(this.rcx),
            rdx: ptrString(this.rdx),
          });
        }
        return;
      }
      const stack = activeStack();
      const popped = stack.pop();
      const after = descriptorSnapshot(this.rcx);
      for (const hit of this.targetHits) bump(targetStringCounts, hit);
      const row = {
        event: 'wrapper_target_leave',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        function_rva: `0x${RVAS.wrapper_142049220.toString(16)}`,
        wrapper_call_index: this.index,
        caller_rva: this.callerRva,
        descriptor_ptr: ptrString(this.rcx),
        descriptor_static_rva: rvaOf(this.rcx),
        string_key: readUtf8(this.rdx, 256),
        rcx: ptrString(this.context.rcx),
        rdx: ptrString(this.context.rdx),
        r8: ptrString(this.context.r8),
        r9: ptrString(this.context.r9),
        target_hits: this.targetHits,
        string_scan_hits: this.stringHits,
        descriptor_before: this.before,
        descriptor_after: after,
        descriptor_field_diff: diffSnapshots(this.before, after),
        helper_events_inside_wrapper: popped ? popped.helper_events : [],
        candidate_registry_map_pointer: null,
        candidate_registry_node_pointer: null,
        return_value: ptrString(retval),
        return_value_equals_descriptor: ptrString(retval) === ptrString(this.rcx),
        return_value_stored_by_caller: false,
      };
      for (const hit of this.targetHits) {
        if (!firstTargetEvents[hit]) firstTargetEvents[hit] = row;
      }
      addBacktrace(row, this.context);
      writeJson(row);
    },
  });
}

function hookHelper(name, rva) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      this.active = currentWrapper();
      this.helperCallIndex = helperIndex++;
      this.name = name;
      if (!this.active) return;
      this.args = {
        rcx: ptrString(this.context.rcx),
        rdx: ptrString(this.context.rdx),
        r8: ptrString(this.context.r8),
        r9: ptrString(this.context.r9),
        rcx_preview: pointerPreview(this.context.rcx),
        rdx_preview: pointerPreview(this.context.rdx),
      };
    },
    onLeave(retval) {
      if (!this.active) return;
      bump(helperCounts, this.name);
      const row = {
        event: 'helper_inside_target_wrapper',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        helper_name: this.name,
        helper_rva: `0x${rva.toString(16)}`,
        helper_call_index: this.helperCallIndex,
        wrapper_call_index: this.active.wrapper_call_index,
        wrapper_target_hits: this.active.target_hits,
        descriptor_ptr: this.active.descriptor_ptr,
        descriptor_static_rva: this.active.descriptor_static_rva,
        string_key: this.active.string_key,
        args: this.args,
        return_value: ptrString(retval),
        return_value_rva: rvaOf(retval),
        candidate_registry_pointer: null,
      };
      this.active.helper_events.push(row);
      writeJson(row);
    },
  });
}

function writeSummary(kind) {
  writeJson({
    event: 'summary',
    kind,
    timestamp_ms: nowMs(),
    process_id: processId,
    module_base: cspBase.toString(),
    output_path: outPath,
    counts,
    target_string_counts: targetStringCounts,
    helper_counts_inside_target_wrappers: helperCounts,
    descriptors,
    candidate_registry_pointers: candidateRegistryPointers,
    first_target_events: firstTargetEvents,
  });
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  hook_rvas: RVAS,
  descriptors,
});

for (const [name, meta] of Object.entries(STUBS)) hookStub(name, meta);
hookWrapper();
hookHelper('descriptor_string_reset_or_dtor_142049920', RVAS.helper_142049920);
hookHelper('descriptor_utf16_storage_reserve_14204A290', RVAS.helper_14204A290);

setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
