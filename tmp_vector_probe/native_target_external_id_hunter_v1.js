// Read-only target external-id route hunter.
//
// Purpose: stop guessing metadata routes. Log the first native places that see
// the exact Vector_SizePressure VectorData external id during process startup or
// command-line file open.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const PREVIEW_ID = 'extrnlid5943B673F7C84B779ED2D7C96E942EAE';
const MAX_EVENTS = 800;
const MAX_BACKTRACES = 120;
const MAX_SCAN = 16 * 1024 * 1024;
const SUMMARY_INTERVAL_MS = 5000;

const KNOWN_RVAS = {
  exta_body_owner_143A41780: 0x3a41780,
  exta_body_caller_a_143A3E1A7_return_site: 0x3a3e1ac,
  exta_body_caller_b_143A3E1D1_return_site: 0x3a3e1d6,
  generic_value_reader_143366080: 0x3366080,
  generic_value_reader_143365f90: 0x3365f90,
  generic_value_reader_143365840: 0x3365840,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_target_external_id_hunter_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let eventIndex = 0;
let backtracesWritten = 0;
const counts = {};
const targetCounts = {};
const callerCounts = {};
const handlePaths = {};
const firstEvents = {};

const asciiPattern = asciiHex(TARGET_ID);
const utf16Pattern = utf16Hex(TARGET_ID);
const previewAsciiPattern = asciiHex(PREVIEW_ID);
const previewUtf16Pattern = utf16Hex(PREVIEW_ID);

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}
function nowMs() { return Date.now(); }
function ptrString(p) { try { return !p || p.isNull() ? null : p.toString(); } catch (_) { return null; } }
function vaOfRva(rva) { return cspBase.add(rva); }
function rvaOf(p) {
  try {
    if (!p || p.isNull()) return null;
    const d = p.sub(cspBase);
    if (d.compare(ptr(0)) < 0 || d.compare(ptr(0x8000000)) > 0) return null;
    return `0x${d.toUInt32().toString(16)}`;
  } catch (_) { return null; }
}
function bump(map, key) { const k = key ?? 'unknown'; map[k] = (map[k] || 0) + 1; }
function writeJson(row) {
  if (eventIndex > MAX_EVENTS && row.event !== 'summary' && row.event !== 'ready') return;
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}
function asciiHex(text) {
  return Array.from(text).map((ch) => ch.charCodeAt(0).toString(16).padStart(2, '0')).join(' ');
}
function utf16Hex(text) {
  const parts = [];
  for (const ch of text) {
    const c = ch.charCodeAt(0);
    parts.push((c & 0xff).toString(16).padStart(2, '0'));
    parts.push(((c >> 8) & 0xff).toString(16).padStart(2, '0'));
  }
  return parts.join(' ');
}
function safeUtf8(p, maxLen) {
  try { if (!p || p.isNull()) return null; return p.readUtf8String(maxLen || 256); }
  catch (_) { try { return p.readCString(maxLen || 256); } catch (_2) { return null; } }
}
function safeUtf16(p, maxChars) {
  try { if (!p || p.isNull()) return null; return p.readUtf16String(maxChars || 128); }
  catch (_) { return null; }
}
function safeU32(p) { try { return p.readU32(); } catch (_) { return null; } }
function safePtr(p) { try { if (!p || p.isNull()) return null; return p.readPointer(); } catch (_) { return null; } }
function previewPtr(p) {
  const p0 = safePtr(p);
  return {
    ptr: ptrString(p),
    rva: rvaOf(p),
    utf8: safeUtf8(p, 256),
    utf16: safeUtf16(p, 128),
    p0: ptrString(p0),
    p0_rva: rvaOf(p0),
    p0_utf8: safeUtf8(p0, 256),
    p0_utf16: safeUtf16(p0, 128),
  };
}
function scanMemory(addr, size) {
  if (!addr || addr.isNull() || !size || size <= 0) return [];
  const n = Math.min(Number(size), MAX_SCAN);
  const hits = [];
  const scans = [
    ['target_ascii', asciiPattern],
    ['target_utf16', utf16Pattern],
    ['preview_ascii', previewAsciiPattern],
    ['preview_utf16', previewUtf16Pattern],
  ];
  for (const [kind, pattern] of scans) {
    try {
      const found = Memory.scanSync(addr, n, pattern);
      for (const row of found.slice(0, 8)) hits.push({ kind, address: ptrString(row.address), offset: row.address.sub(addr).toString() });
    } catch (_) {}
  }
  return hits;
}
function scanPointerStrings(p) {
  const hits = [];
  const u8 = safeUtf8(p, 512);
  const u16 = safeUtf16(p, 256);
  for (const [label, text] of [['utf8', u8], ['utf16', u16]]) {
    if (!text) continue;
    if (text.includes(TARGET_ID)) hits.push({ kind: `target_${label}`, text });
    if (text.includes(PREVIEW_ID)) hits.push({ kind: `preview_${label}`, text });
  }
  return hits;
}
function logHit(event, details, context) {
  bump(counts, event);
  for (const hit of details.hits || []) bump(targetCounts, hit.kind);
  bump(callerCounts, details.caller_rva || 'unknown');
  const row = {
    event,
    timestamp_ms: nowMs(),
    event_index: eventIndex++,
    process_id: processId,
    thread_id: Process.getCurrentThreadId(),
    ...details,
  };
  if (!firstEvents[event]) firstEvents[event] = row;
  if ((details.hits || []).some((h) => String(h.kind).startsWith('target')) && !firstEvents.target_any) {
    firstEvents.target_any = row;
  }
  if (context && backtracesWritten < MAX_BACKTRACES) {
    backtracesWritten++;
    try {
      row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE).slice(0, 24)
        .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
    } catch (_) { row.backtrace = []; }
  }
  writeJson(row);
}
function moduleExport(moduleNames, exportNames) {
  const out = [];
  for (const mod of Process.enumerateModules()) {
    if (moduleNames && !moduleNames.some((needle) => mod.name.toLowerCase().includes(needle))) continue;
    let exports = [];
    try { exports = mod.enumerateExports(); } catch (_) { continue; }
    for (const exp of exports) {
      if (exp.type !== 'function') continue;
      if (exportNames.some((name) => exp.name === name || exp.name.includes(name))) {
        out.push({ module: mod.name, name: exp.name, address: exp.address });
      }
    }
  }
  return out;
}
function hookExport(moduleName, name, callbacks) {
  const addr = Module.findExportByName(moduleName, name);
  if (!addr) return false;
  try { Interceptor.attach(addr, callbacks(addr)); return true; } catch (_) { return false; }
}

function hookFileApis() {
  hookExport('kernel32.dll', 'CreateFileW', () => ({
    onEnter(args) { this.path = safeUtf16(args[0], 1024); },
    onLeave(retval) {
      if (retval && !retval.isNull() && retval.toString() !== '0xffffffffffffffff') {
        handlePaths[retval.toString()] = this.path;
      }
    },
  }));
  hookExport('kernel32.dll', 'ReadFile', () => ({
    onEnter(args) {
      this.handle = args[0].toString();
      this.buf = args[1];
      this.requested = args[2].toUInt32();
      this.bytesReadPtr = args[3];
    },
    onLeave(retval) {
      if (retval.toInt32() === 0) return;
      let n = this.requested;
      const br = safeU32(this.bytesReadPtr);
      if (br !== null) n = br;
      const hits = scanMemory(this.buf, n);
      if (hits.length) {
        logHit('ReadFile_buffer_hit', {
          caller_rva: rvaOf(this.returnAddress),
          handle: this.handle,
          path: handlePaths[this.handle],
          bytes: n,
          buffer: ptrString(this.buf),
          hits,
        }, this.context);
      }
    },
  }));
  hookExport('kernel32.dll', 'MapViewOfFile', () => ({
    onEnter(args) { this.sizeLow = args[4].toUInt32(); },
    onLeave(retval) {
      if (!retval || retval.isNull()) return;
      const size = this.sizeLow || (4 * 1024 * 1024);
      const hits = scanMemory(retval, size);
      if (hits.length) {
        logHit('MapViewOfFile_hit', { caller_rva: rvaOf(this.returnAddress), view: ptrString(retval), size, hits }, this.context);
      }
    },
  }));
}

function hookSqliteExports() {
  const names = [
    'sqlite3_column_text', 'sqlite3_column_text16', 'sqlite3_column_blob',
    'sqlite3_step', 'sqlite3_prepare_v2', 'sqlite3_prepare16_v2',
  ];
  for (const exp of moduleExport(null, names)) {
    try {
      Interceptor.attach(exp.address, {
        onEnter(args) { this.args = [args[0], args[1], args[2], args[3]]; },
        onLeave(retval) {
          const ptrs = [retval, ...this.args];
          let hits = [];
          for (const p of ptrs) hits = hits.concat(scanPointerStrings(p));
          if (hits.length) {
            logHit('sqlite_export_hit', {
              module: exp.module,
              export_name: exp.name,
              caller_rva: rvaOf(this.returnAddress),
              retval: ptrString(retval),
              args: this.args.map(ptrString),
              hits,
            }, this.context);
          }
        },
      });
    } catch (_) {}
  }
}

function hookStringApis() {
  const names = ['strcmp', 'strncmp', 'strstr', 'wcscmp', 'wcsncmp', 'wcsstr', 'memcmp', 'CompareStringOrdinal'];
  for (const exp of moduleExport(['ucrt', 'msvcr', 'vcruntime', 'kernelbase', 'kernel32', 'shlwapi'], names)) {
    try {
      Interceptor.attach(exp.address, {
        onEnter(args) {
          this.args = [args[0], args[1], args[2], args[3]];
          let hits = [];
          for (const p of this.args) hits = hits.concat(scanPointerStrings(p));
          this.hits = hits;
        },
        onLeave(retval) {
          if (this.hits && this.hits.length) {
            logHit('string_api_hit', {
              module: exp.module,
              export_name: exp.name,
              caller_rva: rvaOf(this.returnAddress),
              retval: ptrString(retval),
              args: this.args.map(ptrString),
              hits: this.hits,
            }, this.context);
          }
        },
      });
    } catch (_) {}
  }
}

function hookConversionApis() {
  hookExport('kernel32.dll', 'WideCharToMultiByte', () => ({
    onEnter(args) {
      this.src = args[2];
      this.srcLen = args[3].toInt32();
      this.dst = args[4];
      this.dstLen = args[5].toInt32();
      this.hits = scanPointerStrings(this.src);
    },
    onLeave(retval) {
      let hits = this.hits || [];
      if (this.dst && !this.dst.isNull() && retval.toInt32() > 0) hits = hits.concat(scanPointerStrings(this.dst));
      if (hits.length) logHit('conversion_api_hit', { api: 'WideCharToMultiByte', caller_rva: rvaOf(this.returnAddress), src: ptrString(this.src), dst: ptrString(this.dst), hits }, this.context);
    },
  }));
  hookExport('kernel32.dll', 'MultiByteToWideChar', () => ({
    onEnter(args) {
      this.src = args[2];
      this.dst = args[4];
      this.hits = scanPointerStrings(this.src);
    },
    onLeave(retval) {
      let hits = this.hits || [];
      if (this.dst && !this.dst.isNull() && retval.toInt32() > 0) hits = hits.concat(scanPointerStrings(this.dst));
      if (hits.length) logHit('conversion_api_hit', { api: 'MultiByteToWideChar', caller_rva: rvaOf(this.returnAddress), src: ptrString(this.src), dst: ptrString(this.dst), hits }, this.context);
    },
  }));
}

function hookKnownCspFunctions() {
  for (const [name, rva] of Object.entries(KNOWN_RVAS)) {
    try {
      Interceptor.attach(vaOfRva(rva), {
        onEnter(args) {
          this.args = [this.context.rcx, this.context.rdx, this.context.r8, this.context.r9];
          let hits = [];
          for (const p of this.args) {
            hits = hits.concat(scanPointerStrings(p));
            const p0 = safePtr(p);
            hits = hits.concat(scanPointerStrings(p0));
          }
          this.hits = hits;
        },
        onLeave(retval) {
          let hits = this.hits || [];
          hits = hits.concat(scanPointerStrings(retval));
          const p0 = safePtr(retval);
          hits = hits.concat(scanPointerStrings(p0));
          if (hits.length) {
            logHit('known_csp_function_hit', {
              function_name: name,
              function_rva: `0x${rva.toString(16)}`,
              caller_rva: rvaOf(this.returnAddress),
              retval: ptrString(retval),
              args: this.args.map((p) => previewPtr(p)),
              hits,
            }, this.context);
          }
        },
      });
    } catch (_) {}
  }
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
    target_counts: targetCounts,
    caller_counts: callerCounts,
    first_events: firstEvents,
  });
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  target_id: TARGET_ID,
  preview_id: PREVIEW_ID,
  known_rvas: KNOWN_RVAS,
});

hookFileApis();
hookSqliteExports();
hookStringApis();
hookConversionApis();
hookKnownCspFunctions();

setInterval(() => writeSummary('periodic'), SUMMARY_INTERVAL_MS);
Script.bindWeak(out, () => writeSummary('unload'));
