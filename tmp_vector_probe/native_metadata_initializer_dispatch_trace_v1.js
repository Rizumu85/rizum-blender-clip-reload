// Read-only metadata initializer dispatch trace.
//
// The target entries are CRT/static-initializer style function pointers in
// .rdata. There is no direct CSP code xref to individual entries, so this trace
// hooks the actual registration stubs plus the shared metadata wrapper.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_BACKTRACES = 100;
const MAX_NONTARGET_WRAPPER_SAMPLES = 80;
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  vector_data_stub: 0x0cdf60,
  vector_object_list_stub: 0x139830,
  time_lapse_blob_stub: 0x1396b0,
  metadata_wrapper_142049220: 0x2049220,
};

const TABLE_ENTRIES = {
  VectorData: 0x4317078,
  VectorObjectList: 0x432a448,
  TimeLapseBlob: 0x432a638,
};

const TARGET_STRINGS = ['VectorObjectList', 'VectorData', 'TimeLapseBlob', 'ExternalChunk'];
const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_metadata_initializer_dispatch_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let dispatchInvocationIndex = 0;
let wrapperNonTargetSamples = 0;
let backtracesWritten = 0;
const counts = {};
const targetStringCounts = {};
const calledStubs = {};
const wrapperCallers = {};
const targetWrapperCallers = {};
const descriptorPointers = {};
const firstEvents = {};
const nonTargetWrapperSamples = [];

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
    return '0x' + p.sub(cspBase).toUInt32().toString(16);
  } catch (_) { return null; }
}
function writeJson(row) { out.write(JSON.stringify(row) + '\n'); out.flush(); }
function bump(map, key) { const k = key ?? 'unknown'; map[k] = (map[k] || 0) + 1; }
function readUtf8(p, maxLen) {
  try { if (!p || p.isNull()) return null; return p.readUtf8String(maxLen || 256); }
  catch (_) { try { return p.readCString(maxLen || 256); } catch (_2) { return null; } }
}
function readUtf16(p, maxChars) {
  try { if (!p || p.isNull()) return null; return p.readUtf16String(maxChars || 128); }
  catch (_) { return null; }
}
function safeReadPointer(p) { try { return p.readPointer(); } catch (_) { return null; } }
function safeReadU64(p) { try { return p.readU64().toString(); } catch (_) { return null; } }

function cleanText(text) {
  if (typeof text !== 'string' || text.length === 0 || text.length > 512) return null;
  let ok = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if ((c >= 0x20 && c <= 0x7e) || c === 9 || c === 10 || c === 13) ok++;
  }
  return ok >= Math.min(4, text.length) ? text : null;
}

function previewPointer(p) {
  const directUtf8 = cleanText(readUtf8(p, 256));
  const directUtf16 = cleanText(readUtf16(p, 128));
  const deref = safeReadPointer(p);
  return {
    ptr: ptrString(p),
    utf8: directUtf8,
    utf16: directUtf16,
    p0: ptrString(deref),
    p0_utf8: cleanText(readUtf8(deref, 256)),
    p0_utf16: cleanText(readUtf16(deref, 128)),
    u64_0: safeReadU64(p),
  };
}

function previewArgs(context) {
  return {
    rcx: ptrString(context.rcx),
    rdx: ptrString(context.rdx),
    r8: ptrString(context.r8),
    r9: ptrString(context.r9),
    rcx_preview: previewPointer(context.rcx),
    rdx_preview: previewPointer(context.rdx),
    r8_preview: previewPointer(context.r8),
    r9_preview: previewPointer(context.r9),
  };
}

function findHits(obj) {
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

function tableEntryValue(name) {
  try {
    return ptrString(vaOfRva(TABLE_ENTRIES[name]).readPointer());
  } catch (_) {
    return null;
  }
}

function hookStub(name, rva, tableEntryRva) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      this.dispatchInvocationIndex = dispatchInvocationIndex++;
      this.row = {
        event: 'registration_stub_entry',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        stub_name: name,
        function_rva: `0x${rva.toString(16)}`,
        caller_rva: rvaOf(this.returnAddress),
        dispatch_invocation_index: this.dispatchInvocationIndex,
        table_entry_addr: ptrString(vaOfRva(tableEntryRva)),
        table_entry_rva: `0x${tableEntryRva.toString(16)}`,
        table_entry_value: tableEntryValue(name),
        target_function_rva: `0x${rva.toString(16)}`,
        did_target_equal_VectorObjectList_stub: name === 'VectorObjectList',
        did_target_equal_VectorData_stub: name === 'VectorData',
        did_target_equal_TimeLapseBlob_stub: name === 'TimeLapseBlob',
        rcx: ptrString(this.context.rcx),
        rdx: ptrString(this.context.rdx),
        r8: ptrString(this.context.r8),
        r9: ptrString(this.context.r9),
        string_preview_from_rdx: previewPointer(this.context.rdx),
        descriptor_global_pointer_from_rcx: ptrString(this.context.rcx),
      };
      bump(calledStubs, name);
      bump(counts, `stub_${name}`);
      addBacktrace(this.row, this.context);
      writeJson(this.row);
    },
    onLeave(retval) {
      writeJson({
        event: 'registration_stub_leave',
        timestamp_ms: nowMs(),
        event_index: eventIndex++,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        stub_name: name,
        function_rva: `0x${rva.toString(16)}`,
        dispatch_invocation_index: this.dispatchInvocationIndex,
        return_value: ptrString(retval),
      });
    },
  });
}

function hookWrapper() {
  const rva = RVAS.metadata_wrapper_142049220;
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      this.argsPreview = previewArgs(this.context);
      this.hits = findHits(this.argsPreview);
      this.callerRva = rvaOf(this.returnAddress);
      bump(counts, 'wrapper_142049220');
      bump(wrapperCallers, this.callerRva);
    },
    onLeave(retval) {
      if (this.hits.length) {
        for (const hit of this.hits) {
          bump(targetStringCounts, hit);
          bump(targetWrapperCallers, `${hit}:${this.callerRva}`);
          if (!descriptorPointers[hit]) descriptorPointers[hit] = [];
          const desc = this.argsPreview.rcx;
          if (desc && descriptorPointers[hit].indexOf(desc) < 0) descriptorPointers[hit].push(desc);
        }
        const row = {
          event: 'metadata_wrapper_target_string',
          timestamp_ms: nowMs(),
          event_index: eventIndex++,
          process_id: processId,
          thread_id: Process.getCurrentThreadId(),
          function_rva: `0x${rva.toString(16)}`,
          caller_rva: this.callerRva,
          rcx: this.argsPreview.rcx,
          rdx: this.argsPreview.rdx,
          r8: this.argsPreview.r8,
          r9: this.argsPreview.r9,
          previews: this.argsPreview,
          target_hits: this.hits,
          descriptor_global_pointer_from_rcx: this.argsPreview.rcx,
          return_value: ptrString(retval),
        };
        for (const hit of this.hits) if (!firstEvents[hit]) firstEvents[hit] = row;
        addBacktrace(row, this.context);
        writeJson(row);
      } else if (wrapperNonTargetSamples < MAX_NONTARGET_WRAPPER_SAMPLES) {
        wrapperNonTargetSamples++;
        nonTargetWrapperSamples.push({
          caller_rva: this.callerRva,
          rcx: this.argsPreview.rcx,
          rdx: this.argsPreview.rdx,
          return_value: ptrString(retval),
        });
      }
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
    called_stubs: calledStubs,
    target_string_counts: targetStringCounts,
    wrapper_callers: wrapperCallers,
    target_wrapper_callers: targetWrapperCallers,
    descriptor_pointers: descriptorPointers,
    first_events: firstEvents,
    non_target_wrapper_samples: nonTargetWrapperSamples,
    table_entries: Object.fromEntries(Object.entries(TABLE_ENTRIES).map(([name, rva]) => [name, {
      table_entry_rva: `0x${rva.toString(16)}`,
      table_entry_addr: ptrString(vaOfRva(rva)),
      table_entry_value: tableEntryValue(name),
    }])),
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
  table_entries: TABLE_ENTRIES,
});

hookStub('VectorData', RVAS.vector_data_stub, TABLE_ENTRIES.VectorData);
hookStub('VectorObjectList', RVAS.vector_object_list_stub, TABLE_ENTRIES.VectorObjectList);
hookStub('TimeLapseBlob', RVAS.time_lapse_blob_stub, TABLE_ENTRIES.TimeLapseBlob);
hookWrapper();

setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
