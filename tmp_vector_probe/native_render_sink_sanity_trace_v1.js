// Read-only Frida sanity trace for CSP saved-vector render sinks.
// Does not match dabs, does not filter to suspect samples, and does not patch memory.
// Run with:
//   python -m frida_tools.repl -p <CLIPStudioPaint PID> -l tmp_vector_probe/native_render_sink_sanity_trace_v1.js -q -t inf

'use strict';

const MODULE_NAME = 'CLIPStudioPaint.exe';
const OUT_PATH = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe/native_render_sink_sanity_trace_v1.jsonl';

const FIRST_N_PER_FUNCTION = 80;
const FIRST_N_ROW_WRITER = 200;
const FIRST_N_BACKTRACE = 20;
const SUMMARY_INTERVAL_MS = 5000;

const HOOKS = [
  { key: 'plot_1422D8550', event: 'plot_1422D8550', rva: 0x22D8550, sample: 'plot' },
  { key: 'submit_14255DFE0', event: 'submit_14255DFE0', rva: 0x255DFE0, sample: 'generic' },
  { key: 'submit_14260F550', event: 'submit_14260F550', rva: 0x260F550, sample: 'generic' },
  { key: 'bridge_14260DB90', event: 'bridge_14260DB90', rva: 0x260DB90, sample: 'generic' },
  { key: 'dispatcher_14263F410', event: 'dispatcher_14263F410', rva: 0x263F410, sample: 'dispatcher' },
  { key: 'hard_circle_142640150', event: 'hard_circle_142640150', rva: 0x2640150, sample: 'hard_circle' },
  { key: 'row_writer_14263AC30', event: 'row_writer_14263AC30', rva: 0x263AC30, sample: 'row_writer' },

  // Direct call targets observed inside 0x14263F410 by r2 disassembly.
  { key: 'helper_140D4C6B0', event: 'dispatcher_helper_140D4C6B0', rva: 0x0D4C6B0, sample: 'helper' },
  { key: 'helper_14206C980', event: 'dispatcher_helper_14206C980', rva: 0x206C980, sample: 'helper' },
  { key: 'helper_14206CBB0', event: 'dispatcher_helper_14206CBB0', rva: 0x206CBB0, sample: 'helper' },
  { key: 'helper_14206CBF0', event: 'dispatcher_helper_14206CBF0', rva: 0x206CBF0, sample: 'helper' },
  { key: 'helper_1422DBF30', event: 'dispatcher_helper_1422DBF30', rva: 0x22DBF30, sample: 'helper' },
  { key: 'helper_142637570', event: 'dispatcher_helper_142637570', rva: 0x2637570, sample: 'helper' },
  { key: 'helper_14263E200', event: 'dispatcher_helper_14263E200', rva: 0x263E200, sample: 'helper' },
  { key: 'helper_14263FC50', event: 'dispatcher_helper_14263FC50', rva: 0x263FC50, sample: 'helper' },
  { key: 'helper_142640420', event: 'dispatcher_helper_142640420', rva: 0x2640420, sample: 'helper' },
  { key: 'helper_142640C90', event: 'dispatcher_helper_142640C90', rva: 0x2640C90, sample: 'helper' },
  { key: 'helper_1426427D0', event: 'dispatcher_helper_1426427D0', rva: 0x26427D0, sample: 'helper' },
  { key: 'helper_142644230', event: 'dispatcher_helper_142644230', rva: 0x2644230, sample: 'helper' },
  { key: 'helper_142663A40', event: 'dispatcher_helper_142663A40', rva: 0x2663A40, sample: 'helper' },
  { key: 'helper_142663A80', event: 'dispatcher_helper_142663A80', rva: 0x2663A80, sample: 'helper' },
  { key: 'helper_142664050', event: 'dispatcher_helper_142664050', rva: 0x2664050, sample: 'helper' },
  { key: 'helper_142664260', event: 'dispatcher_helper_142664260', rva: 0x2664260, sample: 'helper' },
  { key: 'helper_142664BF0', event: 'dispatcher_helper_142664BF0', rva: 0x2664BF0, sample: 'helper' },
  { key: 'helper_14266E1E0', event: 'dispatcher_helper_14266E1E0', rva: 0x266E1E0, sample: 'helper' },
];

const moduleBase = Process.getModuleByName(MODULE_NAME).base;
const out = new File(OUT_PATH, 'w');
const counts = {};
const rowWriterCallers = {};
const dispatcherHelperCallers = {};
const firstCallers = {};
let summarySeq = 0;

for (const hook of HOOKS) {
  counts[hook.key] = 0;
  firstCallers[hook.key] = [];
}

function addr(rva) {
  return moduleBase.add(ptr(rva));
}

function rvaOf(p) {
  try {
    if (p === null || p.isNull()) return null;
    return '0x' + p.sub(moduleBase).toUInt32().toString(16);
  } catch (_) {
    return null;
  }
}

function ptrString(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.toString();
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

function readPointer(p, off) {
  try {
    if (p === null || p.isNull()) return null;
    return p.add(off).readPointer().toString();
  } catch (_) {
    return null;
  }
}

function argPointers(args) {
  return {
    rcx: ptrString(args[0]),
    rdx: ptrString(args[1]),
    r8: ptrString(args[2]),
    r9: ptrString(args[3]),
  };
}

function stackSlots(context) {
  const sp = context.rsp;
  return {
    stack_0x20_ptr: readPointer(sp, 0x20),
    stack_0x28_ptr: readPointer(sp, 0x28),
    stack_0x30_ptr: readPointer(sp, 0x30),
    stack_0x38_ptr: readPointer(sp, 0x38),
    stack_0x40_i32: readI32(sp, 0x40),
    stack_0x44_i32: readI32(sp, 0x44),
    stack_0x48_i32: readI32(sp, 0x48),
    stack_0x4c_i32: readI32(sp, 0x4c),
  };
}

function nearbyDoubles(p) {
  const out = [];
  for (let off = 0x180; off <= 0x1e0; off += 8) {
    out.push({ off: '0x' + off.toString(16), value: readDouble(p, off) });
  }
  return out;
}

function addBacktrace(row, context, callIndex) {
  if (callIndex >= FIRST_N_BACKTRACE) return;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 16)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function bumpHistogram(hist, key) {
  const safeKey = key || 'unknown';
  hist[safeKey] = (hist[safeKey] || 0) + 1;
}

function summaryEvent(reason) {
  writeJson({
    event: 'summary',
    reason,
    summary_seq: summarySeq++,
    counters: {
      plot_1422D8550_calls: counts.plot_1422D8550,
      submit_14255DFE0_calls: counts.submit_14255DFE0,
      submit_14260F550_calls: counts.submit_14260F550,
      bridge_14260DB90_calls: counts.bridge_14260DB90,
      dispatcher_14263F410_calls: counts.dispatcher_14263F410,
      hard_circle_142640150_calls: counts.hard_circle_142640150,
      row_writer_14263AC30_calls: counts.row_writer_14263AC30,
    },
    all_counts: counts,
    row_writer_callers: rowWriterCallers,
    dispatcher_helper_callers: dispatcherHelperCallers,
    first_callers: firstCallers,
  });
}

function baseRow(hook, args, context, returnAddress, callIndex) {
  const caller = returnAddress || ptr(0);
  const row = {
    event: hook.event,
    function_key: hook.key,
    function_rva: '0x' + hook.rva.toString(16),
    call_index_per_function: callIndex,
    thread_id: Process.getCurrentThreadId(),
    return_address: ptrString(caller),
    caller_rva: rvaOf(caller),
    args: argPointers(args),
  };
  if (firstCallers[hook.key].length < 20) firstCallers[hook.key].push(row.caller_rva);
  addBacktrace(row, context, callIndex);
  return row;
}

function enrichRow(row, hook, args, context) {
  if (hook.sample === 'plot') {
    row.style_flag_0x78 = readU32(args[0], 0x78);
    row.sample_x = readDouble(args[1], 0x00);
    row.sample_y = readDouble(args[1], 0x08);
    row.sample_primary = readDouble(args[1], 0x28);
    row.sample_secondary = readDouble(args[1], 0x30);
  } else if (hook.sample === 'dispatcher') {
    row.ctx_0x1b0 = readDouble(args[0], 0x1b0);
    row.ctx_0x1b8 = readDouble(args[0], 0x1b8);
    row.ctx_0x1c0 = readDouble(args[0], 0x1c0);
    row.ctx_nearby_doubles_0x180_0x1e0 = nearbyDoubles(args[0]);
  } else if (hook.sample === 'hard_circle') {
    row.raw_context_pointer_rcx = ptrString(args[0]);
    row.rdx = ptrString(args[1]);
    row.ctx_0x1b0 = readDouble(args[0], 0x1b0);
    row.ctx_0x1b8 = readDouble(args[0], 0x1b8);
    row.ctx_0x1c0 = readDouble(args[0], 0x1c0);
    row.ctx_nearby_doubles_0x180_0x1e0 = nearbyDoubles(args[0]);
  } else if (hook.sample === 'row_writer') {
    row.registers = {
      rbx: ptrString(context.rbx),
      rbx_i32: context.rbx.toInt32(),
      rdi: ptrString(context.rdi),
      rdi_i32: context.rdi.toInt32(),
      r8: ptrString(context.r8),
      r8_i32: context.r8.toInt32(),
    };
    Object.assign(row, stackSlots(context));
    bumpHistogram(rowWriterCallers, row.caller_rva);
  } else if (hook.sample === 'helper') {
    bumpHistogram(dispatcherHelperCallers, hook.function_rva + '<-' + (row.caller_rva || 'unknown'));
  }
  return row;
}

for (const hook of HOOKS) {
  try {
    Interceptor.attach(addr(hook.rva), {
      onEnter(args) {
        const callIndex = counts[hook.key]++;
        const limit = hook.sample === 'row_writer' ? FIRST_N_ROW_WRITER : FIRST_N_PER_FUNCTION;
        if (callIndex >= limit) return;
        const row = enrichRow(baseRow(hook, args, this.context, this.returnAddress, callIndex), hook, args, this.context);
        writeJson(row);
      },
    });
  } catch (e) {
    writeJson({
      event: 'hook_error',
      function_key: hook.key,
      function_rva: '0x' + hook.rva.toString(16),
      error: String(e),
    });
  }
}

writeJson({
  event: 'ready',
  module_name: MODULE_NAME,
  module_base: moduleBase.toString(),
  output_path: OUT_PATH,
  hooked_functions: HOOKS.map((h) => ({ key: h.key, rva: '0x' + h.rva.toString(16) })),
});

setInterval(function () {
  summaryEvent('periodic');
}, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    summaryEvent('unload');
    out.flush();
    out.close();
  } catch (_) {
  }
});
