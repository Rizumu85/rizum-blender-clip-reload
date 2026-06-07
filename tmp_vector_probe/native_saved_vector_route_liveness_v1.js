// Read-only Frida route-liveness trace for saved-vector open/rebuild attempts.
// This logs whether known saved-vector/sampler/submit/plot/sink candidates are hit.
// No row-span matching, no renderer changes, no memory patching.

'use strict';

const MODULE_NAME = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_SAMPLES_PER_HOOK = 40;

const HOOKS = [
  { key: 'saved_vector_1425A4100', rva: 0x25A4100 },
  { key: 'sampler_1422CC1E0', rva: 0x22CC1E0 },
  { key: 'submit_14255DFE0', rva: 0x255DFE0 },
  { key: 'plot_1422D8550', rva: 0x22D8550 },
  { key: 'radius_written_1422D8B3B', rva: 0x22D8B3B },
  { key: 'downstream_14260F550', rva: 0x260F550 },
  { key: 'bridge_14260DB90', rva: 0x260DB90 },
  { key: 'dispatcher_14263F410', rva: 0x263F410 },
  { key: 'hard_circle_142640150', rva: 0x2640150 },
  { key: 'row_writer_14263AC30', rva: 0x263AC30 },
];

const moduleBase = Process.getModuleByName(MODULE_NAME).base;
const processId = Process.id;
const timestamp = makeTimestamp();
const outPath = `${OUT_DIR}/native_saved_vector_route_liveness_${timestamp}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');
const counts = {};
const callerCounts = {};

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

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function noteCaller(key, callerRva) {
  const bucket = callerCounts[key] || {};
  const caller = callerRva || 'null';
  bucket[caller] = (bucket[caller] || 0) + 1;
  callerCounts[key] = bucket;
}

function summary(reason) {
  writeJson({
    event: 'summary',
    reason,
    process_id: processId,
    module_base: moduleBase.toString(),
    output_path: outPath,
    timestamp_ms: nowMs(),
    counts,
    caller_counts: callerCounts,
  });
}

writeJson({
  event: 'ready_start',
  process_id: processId,
  module_base: moduleBase.toString(),
  output_path: outPath,
  timestamp_ms: nowMs(),
  hooks: HOOKS.map((hook) => ({
    key: hook.key,
    rva: '0x' + hook.rva.toString(16),
    address: addr(hook.rva).toString(),
  })),
});

for (const hook of HOOKS) {
  counts[hook.key] = 0;
  Interceptor.attach(addr(hook.rva), {
    onEnter(args) {
      counts[hook.key] += 1;
      const callerRva = rvaOf(this.returnAddress);
      noteCaller(hook.key, callerRva);
      if (counts[hook.key] > MAX_SAMPLES_PER_HOOK) return;
      const arg0 = args[0];
      const arg1 = args[1];
      writeJson({
        event: 'hit',
        hook: hook.key,
        call_index: counts[hook.key] - 1,
        process_id: processId,
        thread_id: Process.getCurrentThreadId(),
        timestamp_ms: nowMs(),
        rva: '0x' + hook.rva.toString(16),
        address: addr(hook.rva).toString(),
        return_address: ptrString(this.returnAddress),
        caller_rva: callerRva,
        args: [
          ptrString(args[0]),
          ptrString(args[1]),
          ptrString(args[2]),
          ptrString(args[3]),
        ],
        arg0_u32_0x78: readU32(arg0, 0x78),
        arg0_double_0x00: readDouble(arg0, 0x00),
        arg1_double_0x00: readDouble(arg1, 0x00),
        arg1_double_0x08: readDouble(arg1, 0x08),
      });
    },
  });
  writeJson({
    event: 'hook_installed',
    hook: hook.key,
    rva: '0x' + hook.rva.toString(16),
    address: addr(hook.rva).toString(),
    process_id: processId,
    module_base: moduleBase.toString(),
    timestamp_ms: nowMs(),
  });
}

writeJson({
  event: 'ready_hooks_installed',
  process_id: processId,
  module_base: moduleBase.toString(),
  output_path: outPath,
  timestamp_ms: nowMs(),
  hook_count: HOOKS.length,
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
