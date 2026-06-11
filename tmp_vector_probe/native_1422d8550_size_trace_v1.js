// Read-only Frida tracer for CLIPStudioPaint.exe!0x1422D8550.
// Run while manually exporting Vector_SizePressure.clip to PNG from CSP.

'use strict';

const OUT_PATH = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe/native_1422d8550_size_trace_v1.jsonl';
const MODULE_NAME = 'CLIPStudioPaint.exe';
const RVA_ENTRY = 0x22D8550;
const RVA_EFFECTIVE_SIZE_READY = 0x22D8641;  // xmm7 *= sample+0x38 just before this point/at this instruction.
const RVA_NORMAL_STEP_WRITTEN = 0x22D892D;   // after movsd [r14+8], xmm1.
const RVA_FINAL_STEP_READY = 0x22D89B8;      // all active state+8 branches merged.
const RVA_PLOT_RADIUS_WRITTEN = 0x22D8B3B;   // after movsd [rbp], xmm7.
const FILTER_STYLE_FLAG = 0x1C240;
const MAX_ROWS = 50000;

const moduleBase = Process.getModuleByName(MODULE_NAME).base;
const out = new File(OUT_PATH, 'w');
let nextCallIndex = 0;
let writtenRows = 0;
const activeByThread = new Map();

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

function xmmBestEffort(context, name) {
  try {
    const reg = context[name];
    if (reg === undefined || reg === null) return null;
    return reg.toString();
  } catch (_) {
    return null;
  }
}

function writeJson(row) {
  if (writtenRows >= MAX_ROWS) return;
  writtenRows++;
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

Interceptor.attach(addr(RVA_ENTRY), {
  onEnter(args) {
    const stylePtr = args[0];
    const samplePtr = args[1];
    const styleFlag = readU32(stylePtr, 0x78);
    const sp = this.context.rsp;
    const rec = {
      call_index: nextCallIndex++,
      thread_id: Process.getCurrentThreadId(),
      entry_rva: '0x' + RVA_ENTRY.toString(16),
      return_address: safePtr(this.returnAddress),
      caller: safePtr(this.returnAddress),
      style_ptr: safePtr(stylePtr),
      sample_ptr: safePtr(samplePtr),
      plot_ptr_entry_stack_0x38: safePtr(readPointer(sp, 0x38)),
      feedback_state_ptr_entry_stack_0x40: safePtr(readPointer(sp, 0x40)),
      style_flag_0x78: styleFlag,
      style_flag_matches_sizepressure: styleFlag === FILTER_STYLE_FLAG,
      auto_interval_type_style_0x200: readU32(stylePtr, 0x200),
      center_x_guess_sample_0x00: readDouble(samplePtr, 0x00),
      center_y_guess_sample_0x08: readDouble(samplePtr, 0x08),
      primary_sample_0x10: readDouble(samplePtr, 0x10),
      secondary_sample_0x18: readDouble(samplePtr, 0x18),
      auxiliary_sample_0x20: readDouble(samplePtr, 0x20),
      random_or_aux_sample_0x30: readDouble(samplePtr, 0x30),
      size_factor_sample_0x38: readDouble(samplePtr, 0x38),
      flow_factor_sample_0x40: readDouble(samplePtr, 0x40),
      xmm_entry: {
        xmm0: xmmBestEffort(this.context, 'xmm0'),
        xmm1: xmmBestEffort(this.context, 'xmm1'),
        xmm2: xmmBestEffort(this.context, 'xmm2'),
        xmm7: xmmBestEffort(this.context, 'xmm7'),
        xmm8: xmmBestEffort(this.context, 'xmm8'),
      },
      events: [],
    };
    activeByThread.set(tidKey(), rec);
  },
});

Interceptor.attach(addr(RVA_EFFECTIVE_SIZE_READY), {
  onEnter() {
    const rec = activeByThread.get(tidKey());
    if (!rec) return;
    rec.events.push({
      rva: '0x' + RVA_EFFECTIVE_SIZE_READY.toString(16),
      label: 'effective_size_after_size_effector_times_sample_0x38',
      xmm7_best_effort: xmmBestEffort(this.context, 'xmm7'),
      sample_size_factor_0x38: readDouble(ptr(rec.sample_ptr), 0x38),
    });
  },
});

Interceptor.attach(addr(RVA_NORMAL_STEP_WRITTEN), {
  onEnter() {
    const rec = activeByThread.get(tidKey());
    if (!rec) return;
    const statePtr = this.context.r14;
    rec.feedback_state_ptr_r14 = safePtr(statePtr);
    rec.pre_clamp_feedback_step_state_0x08 = readDouble(statePtr, 0x08);
    rec.events.push({
      rva: '0x' + RVA_NORMAL_STEP_WRITTEN.toString(16),
      label: 'normal_feedback_step_written_state_0x08',
      state_ptr: safePtr(statePtr),
      state_0x08: rec.pre_clamp_feedback_step_state_0x08,
      xmm1_best_effort: xmmBestEffort(this.context, 'xmm1'),
      xmm2_best_effort: xmmBestEffort(this.context, 'xmm2'),
      xmm7_best_effort: xmmBestEffort(this.context, 'xmm7'),
      xmm8_best_effort: xmmBestEffort(this.context, 'xmm8'),
    });
  },
});

Interceptor.attach(addr(RVA_FINAL_STEP_READY), {
  onEnter() {
    const rec = activeByThread.get(tidKey());
    if (!rec) return;
    const statePtr = this.context.r14;
    rec.feedback_state_ptr_r14 = safePtr(statePtr);
    rec.final_next_step_state_0x08 = readDouble(statePtr, 0x08);
    rec.events.push({
      rva: '0x' + RVA_FINAL_STEP_READY.toString(16),
      label: 'final_feedback_step_ready_state_0x08',
      state_ptr: safePtr(statePtr),
      state_0x08: rec.final_next_step_state_0x08,
    });
  },
});

Interceptor.attach(addr(RVA_PLOT_RADIUS_WRITTEN), {
  onEnter() {
    const rec = activeByThread.get(tidKey());
    if (!rec) return;
    const plotPtr = this.context.rbp;
    rec.plot_ptr_rbp = safePtr(plotPtr);
    rec.effective_radius_plot_0x00 = readDouble(plotPtr, 0x00);
    rec.plot_opacity_0x08 = readDouble(plotPtr, 0x08);
    rec.plot_flow_0x10 = readDouble(plotPtr, 0x10);
    rec.plot_thickness_0x18 = readDouble(plotPtr, 0x18);
    rec.plot_rotation_0x20 = readDouble(plotPtr, 0x20);
    rec.plot_antialias_0x44 = readU32(plotPtr, 0x44);
    rec.plot_texture_density_0x48 = readDouble(plotPtr, 0x48);
    rec.events.push({
      rva: '0x' + RVA_PLOT_RADIUS_WRITTEN.toString(16),
      label: 'plot_radius_written_plot_0x00',
      plot_ptr: safePtr(plotPtr),
      plot_0x00: rec.effective_radius_plot_0x00,
      xmm7_best_effort: xmmBestEffort(this.context, 'xmm7'),
    });
    writeJson(rec);
    activeByThread.delete(tidKey());
  },
});

Script.bindWeak(out, function () {
  try {
    out.flush();
    out.close();
  } catch (_) {
  }
});
