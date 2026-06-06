// Read-only Frida positive-control trace for the saved-vector plot path.
// Hooks only CLIPStudioPaint.exe RVA 0x22D8550 and 0x22D8B3B.
// No row-span hooks, no export/cache helper hooks, no memory patching.

'use strict';

const MODULE_NAME = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const RVA_PLOT_ENTRY = 0x22D8550;
const RVA_PLOT_RADIUS_WRITTEN = 0x22D8B3B;
const STYLE_FLAG_SIZEPRESSURE = 0x1C240;

const moduleBase = Process.getModuleByName(MODULE_NAME).base;
const processId = Process.id;
const timestamp = makeTimestamp();
const outPath = `${OUT_DIR}/native_plot_only_fresh_open_control_${timestamp}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let nextRawCallIndex = 0;
let nextSizepressureCallIndex = 0;
let totalRawPlotCalls = 0;
let totalSizepressurePlotCalls = 0;
let firstSizepressureCallIndex = null;
let lastSizepressureCallIndex = null;
const countOtherStyleFlags = {};
const activeByThread = new Map();
const maxStackDepthByThread = {};
let unpairedRadiusWrittenCount = 0;

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

function addr(rva) {
  return moduleBase.add(ptr(rva));
}

function tidKey() {
  return String(Process.getCurrentThreadId());
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
    return '0x' + p.sub(moduleBase).toUInt32().toString(16);
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

function readDouble(p, off) {
  try {
    if (p === null || p.isNull()) return null;
    const v = p.add(off).readDouble();
    return Number.isFinite(v) ? v : null;
  } catch (_) {
    return null;
  }
}

function hexFlag(v) {
  if (typeof v !== 'number') return 'null';
  return '0x' + v.toString(16);
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function activeStackDepths() {
  const depths = {};
  for (const item of activeByThread.entries()) {
    depths[item[0]] = item[1].length;
  }
  return depths;
}

function summary(reason) {
  writeJson({
    event: 'summary',
    reason,
    process_id: processId,
    module_base: moduleBase.toString(),
    output_path: outPath,
    timestamp_ms: nowMs(),
    total_raw_plot_calls: totalRawPlotCalls,
    total_sizepressure_plot_calls: totalSizepressurePlotCalls,
    first_sizepressure_call_index: firstSizepressureCallIndex,
    last_sizepressure_call_index: lastSizepressureCallIndex,
    count_styleFlag_0x1C240: totalSizepressurePlotCalls,
    count_other_styleFlags: countOtherStyleFlags,
    active_stack_depths: activeStackDepths(),
    max_stack_depth_by_thread: maxStackDepthByThread,
    unpaired_radius_written_count: unpairedRadiusWrittenCount,
  });
}

writeJson({
  event: 'ready',
  process_id: processId,
  module_base: moduleBase.toString(),
  output_path: outPath,
  timestamp_ms: nowMs(),
  hooks: [
    { name: 'plot_entry', rva: '0x' + RVA_PLOT_ENTRY.toString(16) },
    { name: 'plot_radius_written', rva: '0x' + RVA_PLOT_RADIUS_WRITTEN.toString(16) },
  ],
});

Interceptor.attach(addr(RVA_PLOT_ENTRY), {
  onEnter(args) {
    const stylePtr = args[0];
    const samplePtr = args[1];
    const styleFlag = readU32(stylePtr, 0x78);
    const rawCallIndex = nextRawCallIndex++;
    totalRawPlotCalls++;

    let sizepressureCallIndex = null;
    if (styleFlag === STYLE_FLAG_SIZEPRESSURE) {
      sizepressureCallIndex = nextSizepressureCallIndex++;
      totalSizepressurePlotCalls++;
      if (firstSizepressureCallIndex === null) firstSizepressureCallIndex = sizepressureCallIndex;
      lastSizepressureCallIndex = sizepressureCallIndex;
    } else {
      const key = hexFlag(styleFlag);
      countOtherStyleFlags[key] = (countOtherStyleFlags[key] || 0) + 1;
    }

    const rec = {
      raw_call_index: rawCallIndex,
      sizepressure_call_index: sizepressureCallIndex,
      thread_id: Process.getCurrentThreadId(),
      timestamp_ms: nowMs(),
      module_base: moduleBase.toString(),
      process_id: processId,
      return_address: ptrString(this.returnAddress),
      caller_rva: rvaOf(this.returnAddress),
      style_ptr: ptrString(stylePtr),
      sample_ptr: ptrString(samplePtr),
      styleFlag_0x78: styleFlag,
      styleFlag_0x78_hex: hexFlag(styleFlag),
      sample_center_x_guess: readDouble(samplePtr, 0x00),
      sample_center_y_guess: readDouble(samplePtr, 0x08),
    };
    const key = tidKey();
    let stack = activeByThread.get(key);
    if (stack === undefined) {
      stack = [];
      activeByThread.set(key, stack);
    }
    stack.push(rec);
    maxStackDepthByThread[key] = Math.max(maxStackDepthByThread[key] || 0, stack.length);
    writeJson(Object.assign({ event: 'plot_entry' }, rec));
  },
});

Interceptor.attach(addr(RVA_PLOT_RADIUS_WRITTEN), {
  onEnter() {
    const key = tidKey();
    const stack = activeByThread.get(key);
    const rec = stack && stack.length > 0 ? stack.pop() : undefined;
    if (stack && stack.length === 0) activeByThread.delete(key);
    if (rec === undefined) unpairedRadiusWrittenCount++;
    const plotPtr = this.context.rbp;
    const row = {
      event: 'plot_radius_written',
      raw_call_index: rec ? rec.raw_call_index : null,
      sizepressure_call_index: rec ? rec.sizepressure_call_index : null,
      thread_id: Process.getCurrentThreadId(),
      timestamp_ms: nowMs(),
      module_base: moduleBase.toString(),
      process_id: processId,
      return_address: ptrString(this.returnAddress),
      caller_rva: rvaOf(this.returnAddress),
      plot_ptr: ptrString(plotPtr),
      plot_radius: readDouble(plotPtr, 0x00),
      sample_center_x_guess: rec ? rec.sample_center_x_guess : null,
      sample_center_y_guess: rec ? rec.sample_center_y_guess : null,
      styleFlag_0x78: rec ? rec.styleFlag_0x78 : null,
      styleFlag_0x78_hex: rec ? rec.styleFlag_0x78_hex : null,
      paired_entry_found: rec !== undefined,
    };
    writeJson(row);
  },
});

setInterval(function () {
  summary('periodic');
}, 5000);

Script.bindWeak(out, function () {
  try {
    summary('unload');
    out.flush();
    out.close();
  } catch (_) {
  }
});
