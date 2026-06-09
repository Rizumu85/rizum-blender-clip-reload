// Read-only SQLite/VectorObjectList trace.
//
// This first tries dynamic sqlite3 exports. CSP 5.0.0 appears to have no PE
// sqlite imports, so the script also hooks the known generic value readers as a
// wrapper fallback census for extrnlid strings.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_VECTOR_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const PREVIEW_CACHE_ID = 'extrnlid5943B673F7C84B779ED2D7C96E942EAE';
const MAX_BACKTRACES = 60;
const MAX_NON_RELEVANT_COLUMNS = 120;
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  value_type_reader: 0x3366080,
  string_len_reader: 0x3365f90,
  string_ptr_reader: 0x3365840,
  known_selector_ptr_return: 0x331eb1a,
  known_selector_pre_call: 0x331eb6c,
  known_selector_post_call: 0x331eb72,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_sqlite_vectorobjectlist_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

const stmtState = {};
const columnNamesByStmt = {};
const recentLengthByThreadObject = {};
const counts = {};
const sqlTextCounts = {};
const extrnlidCounts = {};
const callerCounts = {};
let backtracesWritten = 0;
let columnSampleCount = 0;
let readerIndex = 0;

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }

function ptrString(p) {
  try { return p === null || p.isNull() ? null : p.toString(); } catch (_) { return null; }
}

function rvaOf(p) {
  try {
    if (p === null || p.isNull()) return null;
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

function bumpCount(key) {
  counts[key] = (counts[key] || 0) + 1;
}

function readUtf8(p, maxLen) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf8String(maxLen || 4096);
  } catch (_) {
    try { return p.readCString(maxLen || 4096); } catch (_2) { return null; }
  }
}

function readUtf16(p, maxChars) {
  try {
    if (!p || p.isNull()) return null;
    return p.readUtf16String(maxChars || 4096);
  } catch (_) {
    return null;
  }
}

function readAscii(p, maxLen) {
  try {
    if (!p || p.isNull()) return null;
    const u8 = new Uint8Array(p.readByteArray(maxLen || 96));
    return Array.from(u8).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
  } catch (_) {
    return null;
  }
}

function safeReadU32(p) { try { return p.readU32(); } catch (_) { return null; } }
function safeReadPointer(p) { try { return p.readPointer(); } catch (_) { return null; } }

function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 22)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function isRelevantText(text) {
  const s = String(text || '');
  return s.includes('VectorObjectList') || s.includes('VectorData') || s.includes('ExternalChunk') ||
    s.includes('TimeLapseBlob') || s.includes('Layer') || s.includes('BrushStyle') ||
    s.includes('extrnlid') || s === TARGET_VECTOR_ID || s === PREVIEW_CACHE_ID;
}

function isExtrnlid(text) {
  return typeof text === 'string' && text.startsWith('extrnlid');
}

function classifyId(text) {
  return {
    target_vector_id_seen: text === TARGET_VECTOR_ID,
    preview_cache_id_seen: text === PREVIEW_CACHE_ID,
    starts_with_extrnlid: isExtrnlid(text),
  };
}

function findExport(name) {
  for (const moduleName of ['sqlite3.dll', 'SQLite3.dll', 'CLIPStudioPaint.exe']) {
    try {
      const m = Process.getModuleByName(moduleName);
      if (m.findExportByName) {
        const p = m.findExportByName(name);
        if (p) return { moduleName, ptr: p };
      }
      if (m.getExportByName) return { moduleName, ptr: m.getExportByName(name) };
    } catch (_) {}
  }
  try {
    if (Module.findGlobalExportByName) {
      const p = Module.findGlobalExportByName(name);
      if (p) return { moduleName: 'global', ptr: p };
    }
  } catch (_) {}
  return null;
}

function hookExport(name, callbackFactory) {
  const found = findExport(name);
  if (!found) {
    writeJson({ event: 'sqlite_hook_missing', function: name });
    return false;
  }
  Interceptor.attach(found.ptr, callbackFactory(name, found.moduleName, found.ptr));
  writeJson({ event: 'sqlite_hook_installed', function: name, module: found.moduleName, address: found.ptr.toString() });
  return true;
}

function hookSqliteExports() {
  let installed = 0;
  for (const name of ['sqlite3_prepare_v2', 'sqlite3_prepare16_v2']) {
    if (hookExport(name, function (fn) {
      return {
        onEnter(args) {
          this.db = args[0];
          this.sqlPtr = args[1];
          this.stmtOut = args[3];
          this.callerRva = rvaOf(this.returnAddress);
          this.sqlText = fn.includes('16') ? readUtf16(args[1], 4096) : readUtf8(args[1], 4096);
        },
        onLeave(retval) {
          const stmt = safeReadPointer(this.stmtOut);
          const stmtKey = ptrString(stmt);
          if (stmtKey) stmtState[stmtKey] = { sql: this.sqlText, prepare_caller_rva: this.callerRva };
          if (this.sqlText) bump(sqlTextCounts, this.sqlText.slice(0, 160));
          const relevant = isRelevantText(this.sqlText);
          bumpCount('sqlite_prepare');
          if (!relevant) return;
          const row = {
            event: 'sqlite_prepare',
            timestamp_ms: nowMs(),
            process_id: processId,
            function: fn,
            stmt_ptr: stmtKey,
            db_ptr: ptrString(this.db),
            sql: this.sqlText,
            caller_rva: this.callerRva,
            return_value: retval.toInt32(),
          };
          addBacktrace(row, this.context);
          writeJson(row);
        },
      };
    })) installed++;
  }

  if (hookExport('sqlite3_step', function (fn) {
    return {
      onEnter(args) { this.stmt = args[0]; this.callerRva = rvaOf(this.returnAddress); },
      onLeave(retval) {
        const stmtKey = ptrString(this.stmt);
        const state = stmtState[stmtKey] || {};
        const rc = retval.toInt32();
        bumpCount('sqlite_step');
        if (!isRelevantText(state.sql)) return;
        writeJson({ event: 'sqlite_step', timestamp_ms: nowMs(), process_id: processId, stmt_ptr: stmtKey, sql: state.sql, caller_rva: this.callerRva, return_value: rc });
      },
    };
  })) installed++;

  for (const name of ['sqlite3_column_count', 'sqlite3_column_type', 'sqlite3_column_bytes', 'sqlite3_column_int', 'sqlite3_column_int64']) {
    if (hookExport(name, function (fn) {
      return {
        onEnter(args) { this.stmt = args[0]; this.col = args[1] ? args[1].toInt32() : null; this.callerRva = rvaOf(this.returnAddress); },
        onLeave(retval) {
          const stmtKey = ptrString(this.stmt);
          const state = stmtState[stmtKey] || {};
          if (!isRelevantText(state.sql)) return;
          writeJson({ event: 'sqlite_column_scalar', timestamp_ms: nowMs(), process_id: processId, function: fn, stmt_ptr: stmtKey, sql: state.sql, column_index: this.col, caller_rva: this.callerRva, return_value: retval.toString() });
        },
      };
    })) installed++;
  }

  for (const name of ['sqlite3_column_name', 'sqlite3_column_name16']) {
    if (hookExport(name, function (fn) {
      return {
        onEnter(args) { this.stmt = args[0]; this.col = args[1].toInt32(); this.callerRva = rvaOf(this.returnAddress); },
        onLeave(retval) {
          const stmtKey = ptrString(this.stmt);
          const colName = fn.includes('16') ? readUtf16(retval, 512) : readUtf8(retval, 512);
          if (!columnNamesByStmt[stmtKey]) columnNamesByStmt[stmtKey] = {};
          columnNamesByStmt[stmtKey][this.col] = colName;
          const state = stmtState[stmtKey] || {};
          if (!isRelevantText(state.sql) && !isRelevantText(colName)) return;
          writeJson({ event: 'sqlite_column_name', timestamp_ms: nowMs(), process_id: processId, function: fn, stmt_ptr: stmtKey, sql: state.sql, column_index: this.col, column_name: colName, caller_rva: this.callerRva });
        },
      };
    })) installed++;
  }

  for (const name of ['sqlite3_column_text', 'sqlite3_column_text16', 'sqlite3_column_blob']) {
    if (hookExport(name, function (fn) {
      return {
        onEnter(args) { this.stmt = args[0]; this.col = args[1].toInt32(); this.callerRva = rvaOf(this.returnAddress); },
        onLeave(retval) {
          const stmtKey = ptrString(this.stmt);
          const state = stmtState[stmtKey] || {};
          const colName = (columnNamesByStmt[stmtKey] || {})[this.col] || null;
          const text = fn.includes('16') ? readUtf16(retval, 4096) : readUtf8(retval, 4096);
          const relevant = isRelevantText(state.sql) || isRelevantText(colName) || isRelevantText(text);
          if (!relevant && columnSampleCount++ > MAX_NON_RELEVANT_COLUMNS) return;
          if (isExtrnlid(text)) bump(extrnlidCounts, text);
          const row = {
            event: 'sqlite_column_value',
            timestamp_ms: nowMs(),
            process_id: processId,
            function: fn,
            stmt_ptr: stmtKey,
            sql: state.sql,
            column_index: this.col,
            column_name: colName,
            text_preview: text,
            caller_rva: this.callerRva,
            return_ptr: ptrString(retval),
          };
          Object.assign(row, classifyId(text));
          if (row.starts_with_extrnlid || row.target_vector_id_seen) addBacktrace(row, this.context);
          writeJson(row);
        },
      };
    })) installed++;
  }

  if (hookExport('sqlite3_finalize', function (fn) {
    return {
      onEnter(args) { this.stmt = args[0]; this.callerRva = rvaOf(this.returnAddress); },
      onLeave(retval) {
        const stmtKey = ptrString(this.stmt);
        const state = stmtState[stmtKey] || {};
        if (isRelevantText(state.sql)) writeJson({ event: 'sqlite_finalize', timestamp_ms: nowMs(), process_id: processId, stmt_ptr: stmtKey, sql: state.sql, caller_rva: this.callerRva, return_value: retval.toInt32() });
        delete stmtState[stmtKey];
        delete columnNamesByStmt[stmtKey];
      },
    };
  })) installed++;

  return installed;
}

function recentKey(threadId, objectPtr, indexValue) { return `${threadId}:${objectPtr}:${indexValue}`; }
const recentLen = {};

function hookWrapperFallbacks() {
  Interceptor.attach(vaOfRva(RVAS.string_len_reader), {
    onEnter(args) { this.threadId = Process.getCurrentThreadId(); this.objectPtr = ptrString(args[0]); this.indexValue = args[1].toInt32(); this.callerRva = rvaOf(this.returnAddress); },
    onLeave(retval) {
      const length = retval.toInt32();
      recentLen[recentKey(this.threadId, this.objectPtr, this.indexValue)] = { length, caller_rva: this.callerRva };
      if (length === 0x28) writeJson({ event: 'wrapper_string_length_0x28', timestamp_ms: nowMs(), process_id: processId, thread_id: this.threadId, object_ptr: this.objectPtr, index_value: this.indexValue, caller_rva: this.callerRva, length });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.string_ptr_reader), {
    onEnter(args) { this.threadId = Process.getCurrentThreadId(); this.objectPtr = ptrString(args[0]); this.indexValue = args[1].toInt32(); this.callerRva = rvaOf(this.returnAddress); },
    onLeave(retval) {
      const recent = recentLen[recentKey(this.threadId, this.objectPtr, this.indexValue)] || {};
      const text = readAscii(retval, recent.length || 96);
      if (!isExtrnlid(text)) return;
      bump(extrnlidCounts, text);
      bump(callerCounts, this.callerRva);
      const row = { event: 'wrapper_extrnlid_value', timestamp_ms: nowMs(), process_id: processId, thread_id: this.threadId, object_ptr: this.objectPtr, index_value: this.indexValue, caller_rva: this.callerRva, returned_ptr: ptrString(retval), text, paired_length: recent.length, paired_length_caller_rva: recent.caller_rva };
      Object.assign(row, classifyId(text));
      addBacktrace(row, this.context);
      writeJson(row);
    },
  });

  for (const [rva, name] of [[RVAS.known_selector_ptr_return, 'known_selector_ptr_return'], [RVAS.known_selector_pre_call, 'known_selector_pre_call'], [RVAS.known_selector_post_call, 'known_selector_post_call']]) {
    Interceptor.attach(vaOfRva(rva), {
      onEnter() {
        const text = name === 'known_selector_ptr_return' ? readAscii(this.context.rax, 40) : null;
        const row = { event: name, timestamp_ms: nowMs(), process_id: processId, thread_id: Process.getCurrentThreadId(), rva: '0x' + rva.toString(16), rax: ptrString(this.context.rax), eax: this.context.rax.toInt32(), rcx: ptrString(this.context.rcx), rdx: ptrString(this.context.rdx), r8: ptrString(this.context.r8), r9: ptrString(this.context.r9), text };
        if (text) Object.assign(row, classifyId(text));
        writeJson(row);
      },
    });
  }
}

function rvaMapForJson(values) {
  const result = {};
  for (const key in values) if (Object.prototype.hasOwnProperty.call(values, key)) result[key] = '0x' + values[key].toString(16);
  return result;
}

function writeSummary(reason) {
  writeJson({ event: 'summary', reason, timestamp_ms: nowMs(), process_id: processId, output_path: outPath, counts, sql_text_counts: sqlTextCounts, extrnlid_counts: extrnlidCounts, wrapper_extrnlid_caller_histogram: callerCounts });
}

writeJson({ event: 'ready', timestamp_ms: nowMs(), process_id: processId, module_base: cspBase.toString(), module_path: csp.path, output_path: outPath, target_vector_id: TARGET_VECTOR_ID, preview_cache_id: PREVIEW_CACHE_ID, hook_rvas: rvaMapForJson(RVAS) });
const sqliteHooksInstalled = hookSqliteExports();
hookWrapperFallbacks();
writeJson({ event: 'ready_hooks_installed', timestamp_ms: nowMs(), process_id: processId, output_path: outPath, sqlite_hooks_installed: sqliteHooksInstalled, wrapper_fallback_hooks_installed: true });

setInterval(function () { writeSummary('periodic'); }, SUMMARY_INTERVAL_MS);
Script.bindWeak(out, function () {
  try { writeSummary('unload'); out.flush(); out.close(); } catch (_) {}
});
