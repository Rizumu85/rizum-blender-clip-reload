// Read-only Frida trace for CSP file/mapping/SQLite route discovery.
// Focuses on Vector_SizePressure.clip, related temp/cache paths, and SQLite calls.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_BASENAME = 'vector_sizepressure';
const MAX_BACKTRACES = 30;
const MAX_UNRELATED_CREATEFILE = 200;
const SUMMARY_INTERVAL_MS = 5000;

const cspBase = Process.getModuleByName(CSP_MODULE).base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_file_io_route_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');
const handleToPath = {};
const mappingToSource = {};
const relevantHandles = {};
const counts = {};
const callerCounts = {};
let relevantBacktraces = 0;
let unrelatedCreateFileSamples = 0;
let targetSeen = false;

function makeTimestamp() {
  const d = new Date();
  const pad = (n, width = 2) => String(n).padStart(width, '0');
  return [
    d.getFullYear(),
    pad(d.getMonth() + 1),
    pad(d.getDate()),
    '_',
    pad(d.getHours()),
    pad(d.getMinutes()),
    pad(d.getSeconds()),
    '_',
    pad(d.getMilliseconds(), 3),
  ].join('');
}

function nowMs() {
  return Date.now();
}

function ptrString(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.toString();
  } catch (_) {
    return null;
  }
}

function rvaOf(p) {
  try {
    if (p === null || p.isNull()) return null;
    return '0x' + p.sub(cspBase).toUInt32().toString(16);
  } catch (_) {
    return null;
  }
}

function readUtf16(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.readUtf16String();
  } catch (_) {
    return null;
  }
}

function readU32Ptr(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.readU32();
  } catch (_) {
    return null;
  }
}

function u64FromPair(high, low) {
  return (BigInt(high >>> 0) << 32n) + BigInt(low >>> 0);
}

function bump(name, callerRva) {
  counts[name] = (counts[name] || 0) + 1;
  const caller = callerRva || 'unknown';
  const bucket = callerCounts[name] || {};
  bucket[caller] = (bucket[caller] || 0) + 1;
  callerCounts[name] = bucket;
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function addBacktrace(row, context) {
  if (relevantBacktraces >= MAX_BACKTRACES) return;
  relevantBacktraces++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 18)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function lower(s) {
  return String(s || '').toLowerCase();
}

function isTargetPath(path) {
  const s = lower(path);
  return s.includes(TARGET_BASENAME) || s.endsWith('.clip') || s.includes('rizum-blender-clip-reload');
}

function isPossibleCachePath(path) {
  const s = lower(path);
  return s.includes('\\temp') || s.includes('\\tmp') || s.includes('cache') ||
    s.includes('preview') || s.includes('offscreen') || s.endsWith('.db') ||
    s.endsWith('.sqlite') || s.endsWith('.sqlite3') || s.endsWith('.png') ||
    s.endsWith('.tmp');
}

function shouldLogPath(path) {
  if (isTargetPath(path)) return true;
  if (targetSeen && isPossibleCachePath(path)) return true;
  if (!targetSeen && unrelatedCreateFileSamples < MAX_UNRELATED_CREATEFILE) return true;
  return false;
}

function baseEvent(funcName, context, extra) {
  const callerRva = rvaOf(context.returnAddress);
  bump(funcName, callerRva);
  return Object.assign({
    event: 'api',
    function: funcName,
    timestamp_ms: nowMs(),
    thread_id: Process.getCurrentThreadId(),
    process_id: processId,
    caller: ptrString(context.returnAddress),
    caller_rva: callerRva,
  }, extra || {});
}

function hookExport(moduleNames, exportName, callbacks) {
  for (const moduleName of moduleNames) {
    const p = Module.findExportByName(moduleName, exportName);
    if (p) {
      Interceptor.attach(p, callbacks(exportName, moduleName, p));
      writeJson({ event: 'hook_installed', function: exportName, module: moduleName, address: p.toString() });
      return true;
    }
  }
  writeJson({ event: 'hook_missing', function: exportName, modules: moduleNames });
  return false;
}

function handleKey(h) {
  return ptrString(h);
}

function isInvalidHandle(h) {
  try {
    return h.isNull() || h.toString().toLowerCase() === '0xffffffffffffffff';
  } catch (_) {
    return true;
  }
}

writeJson({
  event: 'ready_start',
  output_path: outPath,
  process_id: processId,
  csp_module_base: cspBase.toString(),
  target_basename: TARGET_BASENAME,
});

hookExport(['KernelBase.dll', 'kernel32.dll'], 'CreateFileW', (name, moduleName) => ({
  onEnter(args) {
    this.path = readUtf16(args[0]);
    this.access = args[1].toString();
    this.share = args[2].toString();
    this.creation = args[4].toString();
    this.flags = args[5].toString();
    this.shouldLog = shouldLogPath(this.path);
    if (this.shouldLog && !isTargetPath(this.path)) unrelatedCreateFileSamples++;
  },
  onLeave(retval) {
    if (!this.shouldLog) return;
    const h = handleKey(retval);
    const targetPath = isTargetPath(this.path);
    if (targetPath) targetSeen = true;
    if (!isInvalidHandle(retval)) {
      handleToPath[h] = this.path;
      if (targetPath || isPossibleCachePath(this.path)) relevantHandles[h] = true;
    }
    const row = baseEvent(name, this, {
      module: moduleName,
      path: this.path,
      handle: h,
      access: this.access,
      share: this.share,
      creation: this.creation,
      flags: this.flags,
      return_value: ptrString(retval),
      relevant_path: targetPath || isPossibleCachePath(this.path),
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'ReadFile', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.path = handleToPath[this.handle] || null;
    this.size = args[2].toUInt32();
    this.bytesReadPtr = args[3];
    this.relevant = relevantHandles[this.handle] || isTargetPath(this.path) || isPossibleCachePath(this.path);
  },
  onLeave(retval) {
    if (!this.relevant) return;
    const row = baseEvent(name, this, {
      module: moduleName,
      path: this.path,
      handle: this.handle,
      size: this.size,
      bytes_read: readU32Ptr(this.bytesReadPtr),
      return_value: retval.toInt32(),
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'SetFilePointerEx', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.path = handleToPath[this.handle] || null;
    this.distanceLow = args[1].toString();
    this.newPosPtr = args[2];
    this.method = args[3].toUInt32();
    this.relevant = relevantHandles[this.handle] || isTargetPath(this.path) || isPossibleCachePath(this.path);
  },
  onLeave(retval) {
    if (!this.relevant) return;
    const row = baseEvent(name, this, {
      module: moduleName,
      path: this.path,
      handle: this.handle,
      distance_low: this.distanceLow,
      move_method: this.method,
      new_position_low: readU32Ptr(this.newPosPtr),
      return_value: retval.toInt32(),
    });
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'CreateFileMappingW', (name, moduleName) => ({
  onEnter(args) {
    this.fileHandle = handleKey(args[0]);
    this.path = handleToPath[this.fileHandle] || null;
    this.protect = args[2].toString();
    this.maxSize = u64FromPair(args[3].toUInt32(), args[4].toUInt32()).toString();
    this.name = readUtf16(args[5]);
    this.relevant = relevantHandles[this.fileHandle] || isTargetPath(this.path) || isPossibleCachePath(this.path);
  },
  onLeave(retval) {
    if (!this.relevant) return;
    const mapping = handleKey(retval);
    mappingToSource[mapping] = { path: this.path, file_handle: this.fileHandle };
    const row = baseEvent(name, this, {
      module: moduleName,
      path: this.path,
      file_handle: this.fileHandle,
      mapping_handle: mapping,
      mapping_name: this.name,
      protect: this.protect,
      max_size: this.maxSize,
      return_value: ptrString(retval),
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'MapViewOfFile', (name, moduleName) => ({
  onEnter(args) {
    this.mapping = handleKey(args[0]);
    this.source = mappingToSource[this.mapping] || null;
    this.access = args[1].toString();
    this.offset = u64FromPair(args[2].toUInt32(), args[3].toUInt32()).toString();
    this.size = args[4].toString();
    this.relevant = this.source !== null;
  },
  onLeave(retval) {
    if (!this.relevant) return;
    const row = baseEvent(name, this, {
      module: moduleName,
      path: this.source.path,
      mapping_handle: this.mapping,
      file_handle: this.source.file_handle,
      view_base: ptrString(retval),
      access: this.access,
      offset: this.offset,
      size: this.size,
      return_value: ptrString(retval),
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'UnmapViewOfFile', (name, moduleName) => ({
  onEnter(args) {
    const row = baseEvent(name, this, {
      module: moduleName,
      view_base: ptrString(args[0]),
    });
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'CloseHandle', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.path = handleToPath[this.handle] || null;
    this.mapping = mappingToSource[this.handle] || null;
    this.relevant = relevantHandles[this.handle] || this.mapping !== null;
  },
  onLeave(retval) {
    if (!this.relevant) return;
    const row = baseEvent(name, this, {
      module: moduleName,
      handle: this.handle,
      path: this.path,
      mapping_source: this.mapping,
      return_value: retval.toInt32(),
    });
    writeJson(row);
    delete handleToPath[this.handle];
    delete relevantHandles[this.handle];
    delete mappingToSource[this.handle];
  },
}));

function installSqliteHooks() {
  for (const m of Process.enumerateModules()) {
    const modName = m.name.toLowerCase();
    if (!modName.includes('sqlite')) continue;
    for (const exportName of ['sqlite3_open', 'sqlite3_open_v2', 'sqlite3_prepare_v2', 'sqlite3_step']) {
      const p = Module.findExportByName(m.name, exportName);
      if (!p) continue;
      Interceptor.attach(p, {
        onEnter(args) {
          const row = baseEvent(exportName, this, {
            module: m.name,
            path: exportName.includes('open') ? readUtf16(args[0]) : null,
            db_or_stmt: ptrString(args[0]),
          });
          addBacktrace(row, this.context);
          writeJson(row);
        },
      });
      writeJson({ event: 'hook_installed', function: exportName, module: m.name, address: p.toString() });
    }
  }
}

installSqliteHooks();

writeJson({
  event: 'ready_hooks_installed',
  output_path: outPath,
  process_id: processId,
  csp_module_base: cspBase.toString(),
});

setInterval(function () {
  writeJson({
    event: 'summary',
    reason: 'periodic',
    timestamp_ms: nowMs(),
    counts,
    caller_counts: callerCounts,
    target_seen: targetSeen,
    tracked_handles: Object.keys(relevantHandles).length,
  });
}, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeJson({
      event: 'summary',
      reason: 'unload',
      timestamp_ms: nowMs(),
      counts,
      caller_counts: callerCounts,
      target_seen: targetSeen,
    });
    out.flush();
    out.close();
  } catch (_) {
  }
});
