// Read-only process-start metadata trace for CSP SQLite/ORM descriptors.
//
// Use with Frida spawn, not late attach. This intentionally avoids renderer
// hooks and records only metadata/string descriptor paths.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_BACKTRACES = 100;
const MAX_NONTARGET_SAMPLES = 80;
const SUMMARY_INTERVAL_MS = 5000;

const TARGET_STRINGS = [
  'VectorObjectList',
  'VectorData',
  'TimeLapseBlob',
  'ExternalChunk',
  'extrnlid',
];

const RVAS = {
  metadata_string_wrapper_142049220: 0x2049220,
  metadata_numeric_wrapper_142040af0: 0x2040af0,
  vector_data_stub_1400cdf60: 0x0cdf60,
  vector_object_list_stub_140139830: 0x139830,
  time_lapse_blob_stub_1401396b0: 0x1396b0,
  post_registration_tail_1438ca758: 0x38ca758,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_sqlite_metadata_spawn_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let backtracesWritten = 0;
let callIndex = 0;
let nonTargetSamples = 0;
const counts = {};
const functionCounts = {};
const targetHitCounts = {};
const callerHistogram = {};
const targetCallerHistogram = {};
const descriptorPointers = {};
const firstEventsByTarget = {};
const nonTargetSampleRows = [];

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }

function ptrString(p) {
  try { return !p || p.isNull() ? null : p.toString(); } catch (_) { return null; }
}

function rvaOf(p) {
  try {
    if (!p || p.isNull()) return null;
    return '0x' + p.sub(cspBase).toUInt32().toString(16);
  } catch (_) {
    return null;
  }
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function bump(map, key, amount) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  map[k] = (map[k] || 0) + (amount || 1);
}

function readUtf8(p, maxLen) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf8String(maxLen || 256);
  } catch (_) {
    try { return p.readCString(maxLen || 256); } catch (_2) { return null; }
  }
}

function readUtf16(p, maxChars) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf16String(maxChars || 128);
  } catch (_) {
    return null;
  }
}

function safeReadPointer(p) { try { return p.readPointer(); } catch (_) { return null; } }
function safeReadU32(p) { try { return p.readU32(); } catch (_) { return null; } }
function safeReadU64(p) { try { return p.readU64().toString(); } catch (_) { return null; } }

function printable(text) {
  if (typeof text !== 'string' || text.length === 0 || text.length > 512) return null;
  let printableCount = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if ((c >= 0x20 && c <= 0x7e) || c === 0x09 || c === 0x0a || c === 0x0d) printableCount++;
  }
  if (printableCount < Math.min(4, text.length)) return null;
  return text;
}

function previewPointer(p, depth) {
  const row = {
    ptr: ptrString(p),
    utf8: printable(readUtf8(p, 256)),
    utf16: printable(readUtf16(p, 128)),
    u32_0: safeReadU32(p),
    u32_4: p ? safeReadU32(p.add(4)) : null,
    u64_0: safeReadU64(p),
    p0: null,
    p8: null,
    fields: [],
  };
  if (!p || p.isNull() || depth <= 0) return row;
  row.p0 = ptrString(safeReadPointer(p));
  row.p8 = ptrString(safeReadPointer(p.add(8)));
  for (let off = 0; off <= 0x80; off += 8) {
    const q = safeReadPointer(p.add(off));
    const t8 = printable(readUtf8(q, 160));
    const t16 = printable(readUtf16(q, 80));
    if (t8 || t16) row.fields.push({ off: `0x${off.toString(16)}`, ptr: ptrString(q), utf8: t8, utf16: t16 });
  }
  return row;
}

function previewArgs(args) {
  const outRows = [];
  for (let i = 0; i < 6; i++) {
    const p = args[i];
    outRows.push({
      index: i,
      value: ptrString(p),
      direct: previewPointer(p, 1),
      deref: previewPointer(safeReadPointer(p), 0),
    });
  }
  return outRows;
}

function flattenTexts(obj, outRows) {
  if (obj === null || obj === undefined) return;
  if (typeof obj === 'string') {
    outRows.push(obj);
    return;
  }
  if (Array.isArray(obj)) {
    for (const item of obj) flattenTexts(item, outRows);
    return;
  }
  if (typeof obj === 'object') {
    for (const key of Object.keys(obj)) flattenTexts(obj[key], outRows);
  }
}

function targetHits(previews) {
  const texts = [];
  flattenTexts(previews, texts);
  const hits = [];
  for (const target of TARGET_STRINGS) {
    if (texts.some((text) => String(text).includes(target))) hits.push(target);
  }
  return hits;
}

function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 24)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function recordTargetMetadata(row, hits) {
  for (const hit of hits) {
    bump(targetHitCounts, hit);
    bump(targetCallerHistogram, `${hit}:${row.function_rva}:${row.caller_rva}`);
    if (!firstEventsByTarget[hit]) firstEventsByTarget[hit] = row;
    if (row.args && row.args[0]) {
      if (!descriptorPointers[hit]) descriptorPointers[hit] = [];
      const ptr = row.args[0];
      if (descriptorPointers[hit].indexOf(ptr) < 0) descriptorPointers[hit].push(ptr);
    }
  }
}

function hookFunction(name, rva) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      this.eventIndex = callIndex++;
      this.name = name;
      this.rva = rva;
      this.callerRva = rvaOf(this.returnAddress);
      this.argsList = [];
      for (let i = 0; i < 6; i++) this.argsList.push(args[i]);
      this.previews = previewArgs(args);
      this.hits = targetHits(this.previews);
      bump(functionCounts, name);
      bump(callerHistogram, `${name}:${this.callerRva}`);
    },
    onLeave(retval) {
      const isTarget = this.hits.length > 0;
      const row = {
        event: isTarget ? 'target_metadata_event' : 'metadata_event_sample',
        event_index: this.eventIndex,
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        name: this.name,
        function_rva: `0x${this.rva.toString(16)}`,
        caller_rva: this.callerRva,
        return_address: ptrString(this.returnAddress),
        args: this.argsList.map(ptrString),
        arg_previews: isTarget ? this.previews : undefined,
        target_hits: this.hits,
        retval: ptrString(retval),
        retval_preview: isTarget ? previewPointer(retval, 1) : undefined,
      };
      if (isTarget) {
        addBacktrace(row, this.context);
        recordTargetMetadata(row, this.hits);
        writeJson(row);
      } else if (nonTargetSamples < MAX_NONTARGET_SAMPLES) {
        nonTargetSamples++;
        nonTargetSampleRows.push({
          event_index: row.event_index,
          name: row.name,
          function_rva: row.function_rva,
          caller_rva: row.caller_rva,
          args: row.args,
          retval: row.retval,
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
    total_calls_per_hook: functionCounts,
    target_string_hit_counts: targetHitCounts,
    descriptor_pointers: descriptorPointers,
    caller_histogram: callerHistogram,
    target_caller_histogram: targetCallerHistogram,
    first_events_by_target: firstEventsByTarget,
    non_target_sample_count: nonTargetSamples,
    non_target_samples: nonTargetSampleRows,
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
});

for (const [name, rva] of Object.entries(RVAS)) {
  hookFunction(name, rva);
}

setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
