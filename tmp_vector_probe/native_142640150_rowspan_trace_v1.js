// Read-only Frida tracer for CLIPStudioPaint.exe!0x142640150 hard no-pattern row spans.
// Capture Vector_SizePressure suspect dabs by correlating 0x1422D8550 call_index
// with 0x142640150 context center/radius after the queued plot path runs.

'use strict';

const MODULE_NAME = 'CLIPStudioPaint.exe';
const OUT_PATH = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe/native_142640150_rowspan_trace_v1.jsonl';

const RVA_PLOT_ENTRY = 0x22D8550;
const RVA_PLOT_RADIUS_WRITTEN = 0x22D8B3B;
const RVA_ROWSPAN_ENTRY = 0x2640150;
const RVA_ROW_WRITE = 0x2640352;  // before mov [rsp+0x20],0x8000 and call 0x14263ac30.

const STYLE_FLAG_SIZEPRESSURE = 0x1C240;
const SUSPECT_DABS = new Set([
  203, 204, 205, 206, 207, 208, 209,
  75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87,
]);
const MATCH_EPS = 1e-7;
const MAX_ROWS = 200000;

const moduleBase = Process.getModuleByName(MODULE_NAME).base;
const out = new File(OUT_PATH, 'w');
let nextRawCallIndex = 0;
let nextSizepressureCallIndex = 0;
let writtenRows = 0;
let totalRawPlotCalls = 0;
let totalSizepressurePlotCalls = 0;
let pendingSuspectDabsAdded = 0;
let pendingSuspectDabsMatched = 0;
const activePlotByThread = new Map();
const pendingSuspectDabs = [];
const activeRowspanByThread = new Map();

function addr(rva) {
  return moduleBase.add(ptr(rva));
}

function tidKey() {
  return Process.getCurrentThreadId().toString();
}

function safePtr(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.toString();
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

function readDouble(p, off) {
  try {
    if (p === null || p.isNull()) return null;
    const v = p.add(off).readDouble();
    return Number.isFinite(v) ? v : null;
  } catch (_) {
    return null;
  }
}

function readPointer(p, off) {
  try {
    if (p === null || p.isNull()) return ptr(0);
    return p.add(off).readPointer();
  } catch (_) {
    return ptr(0);
  }
}

function writeJson(row) {
  if (writtenRows >= MAX_ROWS) return;
  writtenRows++;
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function statsPayload(event) {
  return {
    event,
    total_raw_plot_calls: totalRawPlotCalls,
    total_sizepressure_plot_calls: totalSizepressurePlotCalls,
    pending_suspect_dabs_added: pendingSuspectDabsAdded,
    pending_suspect_dabs_matched: pendingSuspectDabsMatched,
    unmatched_suspect_dabs: pendingSuspectDabs
      .filter((dab) => !dab.used)
      .map((dab) => dab.global_dab_index),
  };
}

function writeStats(event) {
  writeJson(statsPayload(event));
}

function finite(v) {
  return typeof v === 'number' && Number.isFinite(v);
}

function closeEnough(a, b) {
  return finite(a) && finite(b) && Math.abs(a - b) <= MATCH_EPS;
}

function matchPendingDab(cx, cy, radius) {
  let best = null;
  let bestScore = Infinity;
  for (const dab of pendingSuspectDabs) {
    if (dab.used) continue;
    if (!closeEnough(dab.cx, cx) || !closeEnough(dab.cy, cy) || !closeEnough(dab.radius, radius)) {
      continue;
    }
    const score = Math.abs(dab.cx - cx) + Math.abs(dab.cy - cy) + Math.abs(dab.radius - radius);
    if (score < bestScore) {
      best = dab;
      bestScore = score;
    }
  }
  if (best !== null) best.used = true;
  return best;
}

function truncTowardZero(x) {
  return x < 0 ? Math.ceil(x) : Math.floor(x);
}

function recomputeNativeSpan(cx, cy, radius, rowY) {
  if (!finite(cx) || !finite(cy) || !finite(radius) || !finite(rowY)) return {};
  const yCenter = cy - 0.5;
  const dy = rowY - yCenter;
  const spanSq = radius * radius - dy * dy;
  if (!(spanSq > 0)) {
    return { y_center_used: yCenter, dy, span_sq: spanSq };
  }
  const spanBeforeSubtract = Math.sqrt(spanSq);
  const spanAfterSubtract = Math.max(0, spanBeforeSubtract - 0.4);
  return {
    y_center_used: yCenter,
    dy,
    span_sq: spanSq,
    span_before_subtract: spanBeforeSubtract,
    span_after_subtract: spanAfterSubtract,
    subtract_constant: 0.4,
    native_x0_unclipped_recomputed: truncTowardZero(cx - spanAfterSubtract),
    native_x1_unclipped_recomputed: truncTowardZero(cx + spanAfterSubtract),
  };
}

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
    }
    activePlotByThread.set(tidKey(), {
      raw_call_index: rawCallIndex,
      sizepressure_call_index: sizepressureCallIndex,
      style_flag_0x78: styleFlag,
      style_flag_matches_sizepressure: styleFlag === STYLE_FLAG_SIZEPRESSURE,
      sample_ptr: safePtr(samplePtr),
      cx: readDouble(samplePtr, 0x00),
      cy: readDouble(samplePtr, 0x08),
    });
  },
});

Interceptor.attach(addr(RVA_PLOT_RADIUS_WRITTEN), {
  onEnter() {
    const rec = activePlotByThread.get(tidKey());
    if (!rec) return;
    const plotPtr = this.context.rbp;
    rec.plot_ptr = safePtr(plotPtr);
    rec.radius = readDouble(plotPtr, 0x00);
    if (
      rec.style_flag_matches_sizepressure &&
      SUSPECT_DABS.has(rec.sizepressure_call_index) &&
      finite(rec.cx) &&
      finite(rec.cy) &&
      finite(rec.radius)
    ) {
      pendingSuspectDabs.push({
        global_dab_index: rec.sizepressure_call_index,
        raw_call_index: rec.raw_call_index,
        sizepressure_call_index: rec.sizepressure_call_index,
        cx: rec.cx,
        cy: rec.cy,
        radius: rec.radius,
        plot_ptr: rec.plot_ptr,
        used: false,
      });
      pendingSuspectDabsAdded++;
      writeStats('suspect_dab_added');
    }
    activePlotByThread.delete(tidKey());
  },
});

Interceptor.attach(addr(RVA_ROWSPAN_ENTRY), {
  onEnter(args) {
    const contextPtr = args[0];
    const cx = readDouble(contextPtr, 0x1B0);
    const cy = readDouble(contextPtr, 0x1B8);
    const radius = readDouble(contextPtr, 0x1C0);
    const matched = matchPendingDab(cx, cy, radius);
    if (!matched) return;
    pendingSuspectDabsMatched++;
    writeStats('suspect_dab_matched');

    activeRowspanByThread.set(tidKey(), {
      global_dab_index: matched.global_dab_index,
      raw_call_index: matched.raw_call_index,
      sizepressure_call_index: matched.sizepressure_call_index,
      thread_id: Process.getCurrentThreadId(),
      context_ptr: safePtr(contextPtr),
      plot_ptr: matched.plot_ptr,
      cx,
      cy,
      radius,
      entry_rva: '0x' + RVA_ROWSPAN_ENTRY.toString(16),
      clip_left: null,
      clip_top: null,
      clip_right_exclusive: null,
      clip_bottom_exclusive: null,
    });
  },
  onLeave() {
    activeRowspanByThread.delete(tidKey());
  },
});

Interceptor.attach(addr(RVA_ROW_WRITE), {
  onEnter() {
    const rec = activeRowspanByThread.get(tidKey());
    if (!rec) return;
    const sp = this.context.rsp;
    rec.clip_left = readI32(sp, 0x40);
    rec.clip_top = readI32(sp, 0x44);
    rec.clip_right_exclusive = readI32(sp, 0x48);
    rec.clip_bottom_exclusive = readI32(sp, 0x4C);

    const rowY = this.context.rbx.toInt32();
    const x0 = this.context.rdi.toInt32();
    const x1 = this.context.r8.toInt32();
    const computed = recomputeNativeSpan(rec.cx, rec.cy, rec.radius, rowY);
    writeJson(Object.assign({
      event: 'row_span',
      global_dab_index: rec.global_dab_index,
      raw_call_index: rec.raw_call_index,
      sizepressure_call_index: rec.sizepressure_call_index,
      thread_id: rec.thread_id,
      row_y: rowY,
      cx: rec.cx,
      cy: rec.cy,
      radius: rec.radius,
      native_x0_clipped: x0,
      native_x1_clipped: x1,
      inclusive_or_exclusive: 'inclusive_x0_x1_inferred_from_right_minus_1',
      coverage_or_alpha_value: 32768,
      context_ptr: rec.context_ptr,
      plot_ptr: rec.plot_ptr,
      clip_left: rec.clip_left,
      clip_top: rec.clip_top,
      clip_right_exclusive: rec.clip_right_exclusive,
      clip_bottom_exclusive: rec.clip_bottom_exclusive,
      row_write_rva: '0x' + RVA_ROW_WRITE.toString(16),
    }, computed));
  },
});

Script.bindWeak(out, function () {
  try {
    writeStats('trace_summary');
    out.flush();
    out.close();
  } catch (_) {
  }
});
