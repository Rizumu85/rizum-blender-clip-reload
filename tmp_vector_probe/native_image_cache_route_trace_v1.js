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
  const p = findExport(moduleName, exportName);
  if (!p) return;
  installed[key] = true;
  Interceptor.attach(p, factory(exportName, moduleName, p));
  writeJson({ event: 'hook_installed', function: exportName, module: moduleName, address: p.toString() });
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
      this.argValues = [];
      for (let i = 0; i < 12; i++) this.argValues.push(args[i]);
      this.argStrings = this.argValues.map((a) => ptrString(a));
      this.argI32 = this.argValues.map((a) => {
        try {
          return a.toInt32();
        } catch (_) {
          return null;
        }
      });
      this.argU32 = this.argValues.map((a) => {
        try {
          return a.toUInt32();
        } catch (_) {
          return null;
        }
      });
    },
    onLeave(retval) {
      const i32 = this.argI32;
      const u32 = this.argU32;
      const arg = this.argValues;
      let extra = { return_value: ptrString(retval) };
      if (functionName === 'BitBlt') {
        extra = Object.assign(extra, { x: i32[1], y: i32[2], width: i32[3], height: i32[4], rop: this.argStrings[8] });
      } else if (functionName === 'StretchBlt') {
        extra = Object.assign(extra, { x: i32[1], y: i32[2], width: i32[3], height: i32[4], src_width: i32[7], src_height: i32[8], rop: this.argStrings[10] });
      } else if (functionName === 'AlphaBlend') {
        extra = Object.assign(extra, { x: i32[1], y: i32[2], width: i32[3], height: i32[4], src_width: i32[7], src_height: i32[8] });
      } else if (functionName === 'CreateCompatibleBitmap') {
        extra = Object.assign(extra, { width: i32[1], height: i32[2] });
      } else if (functionName === 'CreateDIBSection') {
        const bmi = arg[1];
        extra = Object.assign(extra, {
          width: readI32(bmi, 4),
          height: readI32(bmi, 8),
          planes: readU32(bmi, 12) & 0xffff,
          bit_count: readU32(bmi, 14) & 0xffff,
          compression: readU32(bmi, 16),
          bits_ptr_out: this.argStrings[4],
        });
      } else if (functionName === 'SetDIBits' || functionName === 'GetDIBits') {
        extra = Object.assign(extra, { start_scan: u32[3], scan_lines: u32[4] });
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
      this.argStrings = [ptrString(args[0]), ptrString(args[1]), ptrString(args[2])];
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
        dst: this.argStrings[0],
        src_or_flags: this.argStrings[1],
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
