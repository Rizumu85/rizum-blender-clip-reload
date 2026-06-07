// Read-only Frida trace for broad image/cache/offscreen route discovery.
// Hooks OS image/display APIs and large memory allocation/copy candidates.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_SAMPLES_PER_FUNCTION = 200;
const MAX_BACKTRACE_PER_FUNCTION = 20;
const LARGE_ALLOC_THRESHOLD = 1024 * 1024;
const LARGE_COPY_THRESHOLD = 512 * 1024;

const cspBase = Process.getModuleByName(CSP_MODULE).base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_image_cache_route_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');
const counts = {};
const callerCounts = {};
const backtraceCounts = {};
const installed = {};

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

function readU32(p, off) {
  try {
    if (p === null || p.isNull()) return null;
    return p.add(off).readU32();
  } catch (_) {
    return null;
  }
}

function readI32(p, off) {
  try {
    if (p === null || p.isNull()) return null;
    return p.add(off).readS32();
  } catch (_) {
    return null;
  }
}

function bump(functionName, callerRva) {
  counts[functionName] = (counts[functionName] || 0) + 1;
  const caller = callerRva || 'unknown';
  const bucket = callerCounts[functionName] || {};
  bucket[caller] = (bucket[caller] || 0) + 1;
  callerCounts[functionName] = bucket;
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function addBacktrace(row, context, functionName) {
  const count = backtraceCounts[functionName] || 0;
  if (count >= MAX_BACKTRACE_PER_FUNCTION) return;
  backtraceCounts[functionName] = count + 1;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 18)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function baseRow(functionName, moduleName, context, extra) {
  const callerRva = rvaOf(context.returnAddress);
  bump(functionName, callerRva);
  return Object.assign({
    event: 'api',
    function: functionName,
    module: moduleName,
    timestamp_ms: nowMs(),
    thread_id: Process.getCurrentThreadId(),
    process_id: processId,
    caller: ptrString(context.returnAddress),
    caller_rva: callerRva,
    call_index: counts[functionName] - 1,
  }, extra || {});
}

function shouldSample(functionName) {
  return (counts[functionName] || 0) <= MAX_SAMPLES_PER_FUNCTION;
}

function hookExport(moduleName, exportName, factory) {
  const key = `${moduleName}!${exportName}`;
  if (installed[key]) return;
  const p = Module.findExportByName(moduleName, exportName);
  if (!p) return;
  installed[key] = true;
  Interceptor.attach(p, factory(exportName, moduleName, p));
  writeJson({ event: 'hook_installed', function: exportName, module: moduleName, address: p.toString() });
}

function installHooksForModule(m) {
  const name = m.name;
  const lower = name.toLowerCase();
  if (lower === 'gdi32.dll' || lower === 'gdi32full.dll') {
    for (const fn of ['BitBlt', 'StretchBlt', 'CreateDIBSection', 'CreateCompatibleBitmap', 'SetDIBits', 'GetDIBits']) {
      hookExport(name, fn, imageFactory);
    }
  }
  if (lower === 'msimg32.dll') hookExport(name, 'AlphaBlend', imageFactory);
  if (lower === 'kernelbase.dll' || lower === 'kernel32.dll') {
    for (const fn of ['VirtualAlloc', 'VirtualFree', 'HeapAlloc', 'HeapFree']) hookExport(name, fn, memoryFactory);
  }
  if (lower === 'ntdll.dll') {
    for (const fn of ['RtlAllocateHeap', 'RtlFreeHeap', 'memcpy', 'memmove']) hookExport(name, fn, memoryFactory);
  }
  if (lower === 'msvcrt.dll' || lower.includes('vcruntime') || lower.includes('ucrtbase')) {
    for (const fn of ['memcpy', 'memmove']) hookExport(name, fn, memoryFactory);
  }
  if (lower === 'ole32.dll') hookExport(name, 'CoCreateInstance', comFactory);
  if (lower === 'd2d1.dll') hookExport(name, 'D2D1CreateFactory', factoryFactory);
  if (lower === 'd3d11.dll') hookExport(name, 'D3D11CreateDevice', factoryFactory);
  if (lower === 'd3d9.dll') hookExport(name, 'Direct3DCreate9', factoryFactory);
  if (lower === 'windowscodecs.dll') {
    for (const fn of ['WICCreateImagingFactory_Proxy', 'WICConvertBitmapSource']) hookExport(name, fn, factoryFactory);
  }
}

function imageFactory(functionName, moduleName) {
  return {
    onEnter(args) {
      this.args = args;
    },
    onLeave(retval) {
      const args = this.args;
      let extra = { return_value: ptrString(retval) };
      if (functionName === 'BitBlt') {
        extra = Object.assign(extra, { x: args[1].toInt32(), y: args[2].toInt32(), width: args[3].toInt32(), height: args[4].toInt32(), rop: args[8].toString() });
      } else if (functionName === 'StretchBlt') {
        extra = Object.assign(extra, { x: args[1].toInt32(), y: args[2].toInt32(), width: args[3].toInt32(), height: args[4].toInt32(), src_width: args[7].toInt32(), src_height: args[8].toInt32(), rop: args[10].toString() });
      } else if (functionName === 'AlphaBlend') {
        extra = Object.assign(extra, { x: args[1].toInt32(), y: args[2].toInt32(), width: args[3].toInt32(), height: args[4].toInt32(), src_width: args[7].toInt32(), src_height: args[8].toInt32() });
      } else if (functionName === 'CreateCompatibleBitmap') {
        extra = Object.assign(extra, { width: args[1].toInt32(), height: args[2].toInt32() });
      } else if (functionName === 'CreateDIBSection') {
        const bmi = args[1];
        extra = Object.assign(extra, {
          width: readI32(bmi, 4),
          height: readI32(bmi, 8),
          planes: readU32(bmi, 12) & 0xffff,
          bit_count: readU32(bmi, 14) & 0xffff,
          compression: readU32(bmi, 16),
          bits_ptr_out: ptrString(args[4]),
        });
      } else if (functionName === 'SetDIBits' || functionName === 'GetDIBits') {
        extra = Object.assign(extra, { start_scan: args[3].toUInt32(), scan_lines: args[4].toUInt32() });
      }
      const row = baseRow(functionName, moduleName, this, extra);
      if (shouldSample(functionName)) addBacktrace(row, this.context, functionName);
      if (shouldSample(functionName)) writeJson(row);
    },
  };
}

function memoryFactory(functionName, moduleName) {
  return {
    onEnter(args) {
      this.args = args;
      this.size = 0;
      if (functionName === 'VirtualAlloc') this.size = args[1].toUInt32();
      else if (functionName === 'HeapAlloc' || functionName === 'RtlAllocateHeap') this.size = args[2].toUInt32();
      else if (functionName === 'memcpy' || functionName === 'memmove') this.size = args[2].toUInt32();
      this.largeEnough = this.size >= (functionName.includes('mem') ? LARGE_COPY_THRESHOLD : LARGE_ALLOC_THRESHOLD);
    },
    onLeave(retval) {
      if (!this.largeEnough) return;
      const row = baseRow(functionName, moduleName, this, {
        size: this.size,
        dst: ptrString(this.args[0]),
        src_or_flags: ptrString(this.args[1]),
        return_value: ptrString(retval),
      });
      addBacktrace(row, this.context, functionName);
      writeJson(row);
    },
  };
}

function comFactory(functionName, moduleName) {
  return {
    onEnter(args) {
      this.clsid = ptrString(args[0]);
      this.iid = ptrString(args[3]);
    },
    onLeave(retval) {
      const row = baseRow(functionName, moduleName, this, { clsid_ptr: this.clsid, iid_ptr: this.iid, return_value: retval.toInt32() });
      addBacktrace(row, this.context, functionName);
      writeJson(row);
    },
  };
}

function factoryFactory(functionName, moduleName) {
  return {
    onEnter(args) {
      this.arg0 = ptrString(args[0]);
      this.arg1 = ptrString(args[1]);
      this.arg2 = ptrString(args[2]);
    },
    onLeave(retval) {
      const row = baseRow(functionName, moduleName, this, { arg0: this.arg0, arg1: this.arg1, arg2: this.arg2, return_value: ptrString(retval) });
      addBacktrace(row, this.context, functionName);
      writeJson(row);
    },
  };
}

writeJson({ event: 'ready_start', output_path: outPath, process_id: processId, csp_module_base: cspBase.toString() });
for (const m of Process.enumerateModules()) installHooksForModule(m);
if (Process.attachModuleObserver) {
  Process.attachModuleObserver({
    onAdded(module) {
      installHooksForModule(module);
    },
  });
}
writeJson({ event: 'ready_hooks_installed', output_path: outPath, process_id: processId, hook_count: Object.keys(installed).length });

setInterval(function () {
  writeJson({ event: 'summary', reason: 'periodic', timestamp_ms: nowMs(), counts, caller_counts: callerCounts });
}, 5000);

Script.bindWeak(out, function () {
  try {
    writeJson({ event: 'summary', reason: 'unload', timestamp_ms: nowMs(), counts, caller_counts: callerCounts });
    out.flush();
    out.close();
  } catch (_) {
  }
});
