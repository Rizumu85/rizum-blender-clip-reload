// Read-only Frida trace for the CSP .clip file read route.
// Captures target .clip ReadFile offsets, sizes, and small buffer prefixes.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_NEEDLE = 'vector_sizepressure';
const PREFIX_BYTES = 96;
const MAX_BACKTRACES = 40;
const SUMMARY_INTERVAL_MS = 5000;

const cspBase = Process.getModuleByName(CSP_MODULE).base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_clip_file_read_content_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

const handleToState = {};
const counts = {};
const callerCounts = {};
const magicCounts = {};
let readCallIndex = 0;
let targetReadIndex = 0;
let backtracesWritten = 0;
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

function lower(s) {
  return String(s || '').toLowerCase();
}

function isTargetPath(path) {
  const s = lower(path);
  return s.includes(TARGET_NEEDLE) && s.endsWith('.clip');
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
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 18)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function findExport(moduleName, exportName) {
  try {
    const m = Process.getModuleByName(moduleName);
    if (m.findExportByName) {
      const p = m.findExportByName(exportName);
      if (p) return p;
    }
    if (m.getExportByName) return m.getExportByName(exportName);
  } catch (_) {
  }
  try {
    if (Module.findExportByName) {
      const p = Module.findExportByName(moduleName, exportName);
      if (p) return p;
    }
  } catch (_) {
  }
  try {
    if (Module.findGlobalExportByName) return Module.findGlobalExportByName(exportName);
  } catch (_) {
  }
  try {
    if (Module.getGlobalExportByName) return Module.getGlobalExportByName(exportName);
  } catch (_) {
  }
  return null;
}

function hookExport(moduleNames, exportName, callbacks) {
  for (const moduleName of moduleNames) {
    const p = findExport(moduleName, exportName);
    if (!p) continue;
    Interceptor.attach(p, callbacks(exportName, moduleName, p));
    writeJson({ event: 'hook_installed', function: exportName, module: moduleName, address: p.toString() });
    return true;
  }
  writeJson({ event: 'hook_missing', function: exportName, modules: moduleNames });
  return false;
}

function readPrefix(buffer, length) {
  const n = Math.min(length || 0, PREFIX_BYTES);
  if (!buffer || n <= 0) return { hex: null, ascii: null, magic4: null, probable_type: null };
  try {
    const bytes = buffer.readByteArray(n);
    const u8 = new Uint8Array(bytes);
    const hex = Array.from(u8).map((b) => b.toString(16).padStart(2, '0')).join(' ');
    const ascii = Array.from(u8).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
    const magic4 = Array.from(u8.slice(0, 4)).map((b) => b.toString(16).padStart(2, '0')).join('');
    return { hex, ascii, magic4, probable_type: classifyPrefix(u8, ascii) };
  } catch (_) {
    return { hex: null, ascii: null, magic4: null, probable_type: 'unreadable' };
  }
}

function classifyPrefix(u8, ascii) {
  if (u8.length >= 8 && u8[0] === 0x89 && u8[1] === 0x50 && u8[2] === 0x4e && u8[3] === 0x47) return 'png';
  if (u8.length >= 4 && u8[0] === 0x50 && u8[1] === 0x4b) return 'zip';
  if (u8.length >= 16 && ascii.startsWith('SQLite format 3')) return 'sqlite';
  if (u8.length >= 2 && u8[0] === 0x78 && (u8[1] === 0x01 || u8[1] === 0x9c || u8[1] === 0xda)) return 'zlib';
  if (u8.length >= 2 && u8[0] === 0x1f && u8[1] === 0x8b) return 'gzip';
  if (ascii.startsWith('<?xml') || ascii.startsWith('<')) return 'xmlish';
  if (ascii.startsWith('{') || ascii.startsWith('[')) return 'jsonish';
  if (u8.length && Array.from(u8.slice(0, Math.min(u8.length, 16))).every((b) => b === 0)) return 'zeros';
  return 'binary';
}

function toSignedNumber(p) {
  try {
    const text = p.toString();
    const v = BigInt(text);
    const signed = v > 0x7fffffffffffffffn ? v - 0x10000000000000000n : v;
    if (signed >= BigInt(Number.MIN_SAFE_INTEGER) && signed <= BigInt(Number.MAX_SAFE_INTEGER)) {
      return Number(signed);
    }
  } catch (_) {
  }
  return null;
}

function baseRow(functionName, context, extra) {
  const callerRva = rvaOf(context.returnAddress);
  bump(functionName, callerRva);
  return Object.assign({
    event: 'api',
    function: functionName,
    timestamp_ms: nowMs(),
    thread_id: Process.getCurrentThreadId(),
    process_id: processId,
    caller: ptrString(context.returnAddress),
    caller_rva: callerRva,
  }, extra || {});
}

writeJson({
  event: 'ready_start',
  output_path: outPath,
  process_id: processId,
  csp_module_base: cspBase.toString(),
  target_needle: TARGET_NEEDLE,
});

hookExport(['KernelBase.dll', 'kernel32.dll'], 'CreateFileW', (name, moduleName) => ({
  onEnter(args) {
    this.path = readUtf16(args[0]);
    this.access = args[1].toString();
    this.share = args[2].toString();
    this.creation = args[4].toString();
    this.flags = args[5].toString();
    this.target = isTargetPath(this.path);
  },
  onLeave(retval) {
    if (!this.target) return;
    targetSeen = true;
    const h = handleKey(retval);
    if (!isInvalidHandle(retval)) {
      handleToState[h] = { path: this.path, offset: 0, open_index: counts.CreateFileW || 0 };
    }
    const row = baseRow(name, this, {
      module: moduleName,
      path: this.path,
      handle: h,
      access: this.access,
      share: this.share,
      creation: this.creation,
      flags: this.flags,
      return_value: ptrString(retval),
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'SetFilePointerEx', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.state = handleToState[this.handle] || null;
    this.distance = toSignedNumber(args[1]);
    this.newPosPtr = args[2];
    this.method = args[3].toUInt32();
  },
  onLeave(retval) {
    if (!this.state) return;
    const oldOffset = this.state.offset;
    const newPosLow = readU32Ptr(this.newPosPtr);
    if (this.distance !== null) {
      if (this.method === 0) this.state.offset = this.distance;
      else if (this.method === 1) this.state.offset = Math.max(0, this.state.offset + this.distance);
      else this.state.offset = null;
    } else if (newPosLow !== null) {
      this.state.offset = newPosLow;
    } else {
      this.state.offset = null;
    }
    const row = baseRow(name, this, {
      module: moduleName,
      path: this.state.path,
      handle: this.handle,
      old_offset: oldOffset,
      distance: this.distance,
      move_method: this.method,
      new_position_low: newPosLow,
      tracked_offset_after: this.state.offset,
      return_value: retval.toInt32(),
    });
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'ReadFile', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.state = handleToState[this.handle] || null;
    this.buffer = args[1];
    this.requestedSize = args[2].toUInt32();
    this.bytesReadPtr = args[3];
    this.offsetBefore = this.state ? this.state.offset : null;
    this.rawReadCallIndex = readCallIndex++;
  },
  onLeave(retval) {
    if (!this.state) return;
    const bytesRead = readU32Ptr(this.bytesReadPtr);
    const prefix = readPrefix(this.buffer, bytesRead || 0);
    const localTargetReadIndex = targetReadIndex++;
    if (prefix.probable_type) magicCounts[prefix.probable_type] = (magicCounts[prefix.probable_type] || 0) + 1;
    if (typeof bytesRead === 'number' && typeof this.state.offset === 'number') {
      this.state.offset += bytesRead;
    } else {
      this.state.offset = null;
    }
    const row = baseRow(name, this, {
      module: moduleName,
      path: this.state.path,
      handle: this.handle,
      raw_read_call_index: this.rawReadCallIndex,
      target_read_index: localTargetReadIndex,
      offset_before: this.offsetBefore,
      requested_size: this.requestedSize,
      bytes_read: bytesRead,
      tracked_offset_after: this.state.offset,
      return_value: retval.toInt32(),
      buffer_ptr: ptrString(this.buffer),
      buffer_prefix_hex: prefix.hex,
      buffer_prefix_ascii: prefix.ascii,
      magic4: prefix.magic4,
      probable_type: prefix.probable_type,
    });
    addBacktrace(row, this.context);
    writeJson(row);
  },
}));

hookExport(['KernelBase.dll', 'kernel32.dll'], 'CloseHandle', (name, moduleName) => ({
  onEnter(args) {
    this.handle = handleKey(args[0]);
    this.state = handleToState[this.handle] || null;
  },
  onLeave(retval) {
    if (!this.state) return;
    const row = baseRow(name, this, {
      module: moduleName,
      path: this.state.path,
      handle: this.handle,
      final_tracked_offset: this.state.offset,
      return_value: retval.toInt32(),
    });
    writeJson(row);
    delete handleToState[this.handle];
  },
}));

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
    magic_counts: magicCounts,
    target_seen: targetSeen,
    open_target_handles: Object.keys(handleToState).length,
    target_read_count: targetReadIndex,
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
      magic_counts: magicCounts,
      target_seen: targetSeen,
      target_read_count: targetReadIndex,
    });
    out.flush();
    out.close();
  } catch (_) {
  }
});
