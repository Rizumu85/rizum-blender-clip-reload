// Read-only table/column metadata trace for the CSP SQLite/ORM schema path.
//
// This does not hook rendering. It watches the short schema-registration stubs
// and their common wrapper functions so VectorObjectList/VectorData descriptor
// pointers can be correlated with later row-consumer traces.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_BACKTRACES = 50;
const SUMMARY_INTERVAL_MS = 5000;

const WATCH_TEXT = [
  'VectorObjectList',
  'VectorData',
  'TimeLapseBlob',
  'ExternalChunk',
  'extrnlid',
];

const RVAS = {
  metadata_string_wrapper: 0x2049220,
  metadata_numeric_wrapper: 0x2040af0,
  stub_vector_data: 0x0cdf60,
  stub_vector_object_list: 0x139830,
  stub_time_lapse_blob: 0x1396b0,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_sqlite_table_metadata_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

const counts = {};
const relevantCounts = {};
const callerCounts = {};
const descriptorNames = {};
let backtracesWritten = 0;
let wrapperCallIndex = 0;

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

function bump(map, key) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  map[k] = (map[k] || 0) + 1;
}

function bumpCount(key) { bump(counts, key); }

function readUtf8(p, maxLen) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf8String(maxLen || 512);
  } catch (_) {
    try { return p.readCString(maxLen || 512); } catch (_2) { return null; }
  }
}

function readUtf16(p, maxChars) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf16String(maxChars || 256);
  } catch (_) {
    return null;
  }
}

function safeReadPointer(p) { try { return p.readPointer(); } catch (_) { return null; } }
function safeReadU32(p) { try { return p.readU32(); } catch (_) { return null; } }
function safeReadU64(p) { try { return p.readU64().toString(); } catch (_) { return null; } }

function previewPointer(p) {
  const text8 = readUtf8(p, 160);
  const text16 = readUtf16(p, 80);
  return {
    ptr: ptrString(p),
    utf8: plausibleText(text8) ? text8 : null,
    utf16: plausibleText(text16) ? text16 : null,
    p0: ptrString(safeReadPointer(p)),
    u32_0: safeReadU32(p),
    u32_4: safeReadU32(p ? p.add(4) : p),
    u64_0: safeReadU64(p),
  };
}

function plausibleText(text) {
  if (typeof text !== 'string' || text.length === 0 || text.length > 256) return false;
  let printable = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c >= 0x20 && c <= 0x7e) printable++;
  }
  return printable >= Math.min(text.length, 4);
}

function isRelevantText(text) {
  const s = String(text || '');
  return WATCH_TEXT.some((needle) => s.indexOf(needle) >= 0);
}

function relevantArgStrings(argPreviews) {
  const hits = [];
  for (let i = 0; i < argPreviews.length; i++) {
    for (const kind of ['utf8', 'utf16']) {
      const text = argPreviews[i][kind];
      if (isRelevantText(text)) hits.push({ index: i, kind, text });
    }
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

function summarizeDescriptor(descriptorPtr) {
  if (!descriptorPtr || descriptorPtr.isNull()) return null;
  const fields = [];
  for (let off = 0; off <= 0x80; off += 8) {
    const p = safeReadPointer(descriptorPtr.add(off));
    const text = readUtf8(p, 96) || readUtf16(p, 48);
    if (isRelevantText(text)) fields.push({ offset: `0x${off.toString(16)}`, ptr: ptrString(p), text });
  }
  return {
    ptr: ptrString(descriptorPtr),
    u32_0: safeReadU32(descriptorPtr),
    u32_4: safeReadU32(descriptorPtr.add(4)),
    relevant_pointer_fields: fields,
  };
}

function hookStub(name, rva) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      bumpCount(`stub_${name}_entry`);
      writeJson({
        event: 'metadata_stub_entry',
        name,
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        function_rva: `0x${rva.toString(16)}`,
        caller_rva: rvaOf(this.returnAddress),
        return_address: ptrString(this.returnAddress),
      });
    },
  });
}

function hookWrapper(name, rva) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      this.callIndex = wrapperCallIndex++;
      this.name = name;
      this.rva = rva;
      this.argsList = [];
      for (let i = 0; i < 6; i++) this.argsList.push(args[i]);
      this.argPreviews = this.argsList.map(previewPointer);
      this.hits = relevantArgStrings(this.argPreviews);
      this.callerRva = rvaOf(this.returnAddress);
      this.relevant = this.hits.length > 0;
      bumpCount(`wrapper_${name}_entry`);
      bump(callerCounts, `${name}:${this.callerRva}`);
      if (this.relevant) {
        for (const hit of this.hits) {
          bump(relevantCounts, hit.text);
          descriptorNames[ptrString(args[0])] = hit.text;
        }
      }
    },
    onLeave(retval) {
      if (!this.relevant) return;
      const descriptorPtr = this.argsList[0];
      const row = {
        event: 'metadata_wrapper_call',
        wrapper: this.name,
        call_index: this.callIndex,
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        function_rva: `0x${this.rva.toString(16)}`,
        caller_rva: this.callerRva,
        return_address: ptrString(this.returnAddress),
        args: this.argsList.map(ptrString),
        arg_previews: this.argPreviews,
        relevant_strings: this.hits,
        descriptor_ptr: ptrString(descriptorPtr),
        descriptor_summary: summarizeDescriptor(descriptorPtr),
        retval: ptrString(retval),
      };
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
    relevant_counts: relevantCounts,
    caller_counts: callerCounts,
    descriptor_names: descriptorNames,
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

hookWrapper('metadata_string_wrapper_142049220', RVAS.metadata_string_wrapper);
hookWrapper('metadata_numeric_wrapper_142040af0', RVAS.metadata_numeric_wrapper);
hookStub('VectorData', RVAS.stub_vector_data);
hookStub('VectorObjectList', RVAS.stub_vector_object_list);
hookStub('TimeLapseBlob', RVAS.stub_time_lapse_blob);

setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
