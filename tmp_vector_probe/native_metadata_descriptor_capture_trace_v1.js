// Read-only startup descriptor capture trace for CSP metadata registration.
//
// Hooks only confirmed registration stubs and 0x142049220. It captures
// descriptor/global bytes before and after relevant wrapper calls.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const SNAPSHOT_SIZE = 0x100;
const SUMMARY_INTERVAL_MS = 5000;
const MAX_BACKTRACES = 80;
const MAX_OTHER_WRAPPER_SAMPLES = 40;

const RVAS = {
  VectorData_stub: 0x0cdf60,
  VectorObjectList_stub: 0x139830,
  TimeLapseBlob_stub: 0x1396b0,
  wrapper_142049220: 0x2049220,
};

const STUBS = {
  VectorData: { stub: 0x0cdf60, call: 0x0cdf72, descriptor: 0x5486110, string: 0x44a76a8 },
  VectorObjectList: { stub: 0x139830, call: 0x139842, descriptor: 0x54f2be8, string: 0x44a76b8 },
  TimeLapseBlob: { stub: 0x1396b0, call: 0x1396c2, descriptor: 0x54f3788, string: 0x44a77d8 },
};

const TARGET_STRINGS = ['VectorData', 'VectorObjectList', 'TimeLapseBlob', 'ExternalChunk'];
const TARGET_CALLERS = new Set(Object.values(STUBS).map((item) => `0x${item.call.toString(16)}`));

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_metadata_descriptor_capture_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let wrapperIndex = 0;
let backtracesWritten = 0;
let otherWrapperSamples = 0;
const counts = {};
const targetStringCounts = {};
const descriptors = {};
const wrapperCalls = {};
const stubCalls = {};
const firstTargetEvents = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}
function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }
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
function descriptorSnapshot(p) {
  if (!p || p.isNull()) return null;
  const fields = [];
  for (let off = 0; off < SNAPSHOT_SIZE; off += 8) {
    const q = p.add(off);
    const pp = safeReadPointer(q);
    fields.push({
      offset: `0x${off.toString(16)}`,
      u64: safeReadU64(q),
      u32: safeReadU32(q),
      ptr: ptrString(pp),
      ptr_rva: rvaOf(pp),
      ptr_utf8: readUtf8(pp, 96),
      ptr_utf16: readUtf16(pp, 64),
    });
  }
  return {
    ptr: ptrString(p),
    rva: rvaOf(p),
    bytes_hex: safeBytes(p, SNAPSHOT_SIZE),
    fields,
  };
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
function findTargetHits(obj) {
  const text = JSON.stringify(obj);
  return TARGET_STRINGS.filter((needle) => text.includes(needle));
}
function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 24)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) { row.backtrace = []; }
}

function hookStub(name, meta) {
  const descPtr = vaOfRva(meta.descriptor);
  descriptors[name] = { pointer: ptrString(descPtr), rva: `0x${meta.descriptor.toString(16)}` };
  Interceptor.attach(vaOfRva(meta.stub), {
    onEnter(args) {
      this.name = name;
      this.before = descriptorSnapshot(descPtr);
      bump(counts, 'stub_entry');
      bump(stubCalls, name);
      writeJson({
        event: 'stub_entry',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        stub_name: name,
        stub_rva: `0x${meta.stub.toString(16)}`,
        caller_rva: rvaOf(this.returnAddress),
        descriptor_pointer: ptrString(descPtr),
        descriptor_rva: `0x${meta.descriptor.toString(16)}`,
        descriptor_snapshot_before_stub: this.before,
      });
    },
    onLeave(retval) {
      const after = descriptorSnapshot(descPtr);
      writeJson({
        event: 'stub_leave',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        stub_name: name,
        descriptor_pointer: ptrString(descPtr),
        descriptor_rva: `0x${meta.descriptor.toString(16)}`,
        descriptor_snapshot_after_stub: after,
        descriptor_diff_in_stub: diffSnapshots(this.before, after),
        return_value: ptrString(retval),
      });
    },
  });
}

function hookWrapper() {
  Interceptor.attach(vaOfRva(RVAS.wrapper_142049220), {
    onEnter(args) {
      this.index = wrapperIndex++;
      this.callerRva = rvaOf(this.returnAddress);
      this.rcx = this.context.rcx;
      this.rdx = this.context.rdx;
      this.argsPreview = {
        rcx: pointerPreview(this.context.rcx),
        rdx: pointerPreview(this.context.rdx),
        r8: pointerPreview(this.context.r8),
        r9: pointerPreview(this.context.r9),
      };
      this.hits = findTargetHits(this.argsPreview);
      this.relevant = TARGET_CALLERS.has(this.callerRva) || this.hits.length > 0;
      bump(counts, 'wrapper_entry');
      bump(wrapperCalls, this.callerRva);
      if (this.relevant) this.before = descriptorSnapshot(this.rcx);
    },
    onLeave(retval) {
      if (!this.relevant) {
        if (otherWrapperSamples < MAX_OTHER_WRAPPER_SAMPLES) {
          otherWrapperSamples++;
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
      const after = descriptorSnapshot(this.rcx);
      for (const hit of this.hits) {
        bump(targetStringCounts, hit);
      }
      const row = {
        event: 'wrapper_relevant_leave',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        wrapper_call_index: this.index,
        function_rva: `0x${RVAS.wrapper_142049220.toString(16)}`,
        caller_rva: this.callerRva,
        rcx_descriptor_pointer: ptrString(this.rcx),
        rcx_descriptor_rva: rvaOf(this.rcx),
        rdx_string_pointer: ptrString(this.rdx),
        rdx_string_rva: rvaOf(this.rdx),
        string_ascii_preview: readUtf8(this.rdx, 256),
        string_utf16_preview: readUtf16(this.rdx, 128),
        r8: ptrString(this.context.r8),
        r9: ptrString(this.context.r9),
        return_value: ptrString(retval),
        target_hits: this.hits,
        descriptor_before_call: this.before,
        descriptor_after_call: after,
        descriptor_field_diffs: diffSnapshots(this.before, after),
        descriptor_contains_string_pointer_or_copy: JSON.stringify(after || {}).includes(readUtf8(this.rdx, 256) || '__no_string__'),
        args_preview: this.argsPreview,
      };
      for (const hit of this.hits) {
        if (!firstTargetEvents[hit]) firstTargetEvents[hit] = row;
      }
      addBacktrace(row, this.context);
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
    stub_calls: stubCalls,
    wrapper_callers: wrapperCalls,
    target_string_counts: targetStringCounts,
    descriptors,
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
  descriptors: Object.fromEntries(Object.entries(STUBS).map(([name, meta]) => [name, {
    descriptor_rva: `0x${meta.descriptor.toString(16)}`,
    descriptor_pointer: ptrString(vaOfRva(meta.descriptor)),
    stub_rva: `0x${meta.stub.toString(16)}`,
    wrapper_call_rva: `0x${meta.call.toString(16)}`,
  }])),
});

for (const [name, meta] of Object.entries(STUBS)) hookStub(name, meta);
hookWrapper();
setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
