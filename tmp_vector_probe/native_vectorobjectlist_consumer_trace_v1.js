// Narrow VectorObjectList consumer trace.
//
// This script is intentionally inert until CANDIDATE_RVAS is populated after
// native_sqlite_vectorobjectlist_trace_v1 identifies the actual caller/wrapper
// that reads VectorObjectList.VectorData.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_VECTOR_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const CANDIDATE_RVAS = [
  // Fill after v1 identifies the VectorObjectList.VectorData consumer.
];

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_vectorobjectlist_consumer_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }
function ptrString(p) { try { return p === null || p.isNull() ? null : p.toString(); } catch (_) { return null; } }
function rvaOf(p) { try { return p === null || p.isNull() ? null : '0x' + p.sub(cspBase).toUInt32().toString(16); } catch (_) { return null; } }
function writeJson(row) { out.write(JSON.stringify(row) + '\n'); out.flush(); }

function readAscii(p, maxLen) {
  try {
    if (!p || p.isNull()) return null;
    const u8 = new Uint8Array(p.readByteArray(maxLen || 96));
    return Array.from(u8).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
  } catch (_) {
    return null;
  }
}

function addBacktrace(row, context) {
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 24)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  target_vector_id: TARGET_VECTOR_ID,
  candidate_rvas: CANDIDATE_RVAS.map((rva) => '0x' + rva.toString(16)),
  inert_until_candidate_rvas_are_populated: CANDIDATE_RVAS.length === 0,
});

for (const rva of CANDIDATE_RVAS) {
  Interceptor.attach(vaOfRva(rva), {
    onEnter(args) {
      const possibleTexts = [readAscii(args[0], 96), readAscii(args[1], 96), readAscii(args[2], 96), readAscii(args[3], 96)];
      const hit = possibleTexts.find((text) => text === TARGET_VECTOR_ID || (text || '').startsWith('extrnlid'));
      const row = {
        event: 'vectorobjectlist_consumer_candidate_entry',
        timestamp_ms: nowMs(),
        process_id: processId,
        caller_function_rva: '0x' + rva.toString(16),
        return_address: ptrString(this.returnAddress),
        return_address_rva: rvaOf(this.returnAddress),
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: ptrString(args[3]),
        possible_texts: possibleTexts,
        vector_data_external_id: hit || null,
        target_vector_id_seen: hit === TARGET_VECTOR_ID,
      };
      if (hit === TARGET_VECTOR_ID) addBacktrace(row, this.context);
      writeJson(row);
    },
  });
}

writeJson({ event: 'ready_hooks_installed', timestamp_ms: nowMs(), process_id: processId, output_path: outPath, installed_candidate_count: CANDIDATE_RVAS.length });

Script.bindWeak(out, function () {
  try { out.flush(); out.close(); } catch (_) {}
});
