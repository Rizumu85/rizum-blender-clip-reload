// Read-only census trace for generic value/string readers that return external ids.
//
// Goal: discover which caller reads VectorObjectList.VectorData's external id.
// This intentionally treats 0x143365840 as a generic census point, not as a
// final target.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const PREVIEW_CACHE_ID = 'extrnlid5943B673F7C84B779ED2D7C96E942EAE';
const TARGET_VECTOR_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const MAX_BACKTRACES = 50;
const MAX_NON_EXT_SAMPLES = 80;
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  value_type_reader: 0x3366080,
  string_len_reader: 0x3365f90,
  string_ptr_reader: 0x3365840,
  selector_type_call_return: 0x331ea2f,
  selector_len_call_return: 0x331eaeb,
  selector_ptr_call_return: 0x331eb1a,
  selector_pre_external_call: 0x331eb6c,
  selector_post_external_call: 0x331eb72,
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_external_id_value_reader_census_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let valueReaderCallIndex = 0;
let backtracesWritten = 0;
let nonExtSamplesWritten = 0;
const counts = {};
const callerCounts = {};
const enclosingCounts = {};
const externalIdCounts = {};
const recentByThreadObject = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function vaOfRva(rva) { return cspBase.add(rva); }

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

function readBytes(ptrValue, length) {
  if (!ptrValue || length <= 0) return null;
  try {
    return new Uint8Array(ptrValue.readByteArray(length));
  } catch (_) {
    return null;
  }
}

function toAscii(u8, maxBytes) {
  if (!u8) return null;
  const n = Math.min(u8.length, maxBytes || u8.length);
  return Array.from(u8.slice(0, n)).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
}

function readAscii(ptrValue, maxLen) {
  return toAscii(readBytes(ptrValue, maxLen), maxLen);
}

function safeReadPointer(p) {
  try { return p.readPointer(); } catch (_) { return null; }
}

function safeReadU32(p) {
  try { return p.readU32(); } catch (_) { return null; }
}

function safeFields(base, maxOff) {
  if (!base || base.isNull()) return null;
  const rows = [];
  for (let off = 0; off <= maxOff; off += 8) {
    const pp = safeReadPointer(base.add(off));
    rows.push({
      offset: off,
      ptr: ptrString(pp),
      ptr_rva: rvaOf(pp),
      u32: safeReadU32(base.add(off)),
    });
  }
  return rows;
}

function threadObjectKey(threadId, objectPtr, indexValue) {
  return `${threadId}:${objectPtr || 'null'}:${indexValue}`;
}

function enclosingForCaller(callerRva) {
  if (!callerRva) return null;
  const v = parseInt(callerRva, 16);
  if (v >= 0x331e9f0 && v <= 0x331ebdd) return '0x331e9f0';
  return null;
}

function isExtrnlid(ascii) {
  return typeof ascii === 'string' && ascii.startsWith('extrnlid');
}

function idFlags(ascii, len) {
  return {
    starts_with_extrnlid: isExtrnlid(ascii),
    length_is_0x28: len === 0x28 || len === 40,
    is_preview_cache_id: ascii === PREVIEW_CACHE_ID,
    is_target_vector_id: ascii === TARGET_VECTOR_ID,
  };
}

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

function remember(threadId, objectPtr, indexValue, patch) {
  const key = threadObjectKey(threadId, objectPtr, indexValue);
  if (!recentByThreadObject[key]) recentByThreadObject[key] = {};
  Object.assign(recentByThreadObject[key], patch);
}

function recall(threadId, objectPtr, indexValue) {
  return recentByThreadObject[threadObjectKey(threadId, objectPtr, indexValue)] || {};
}

function rvaMapForJson(values) {
  const result = {};
  for (const key in values) {
    if (Object.prototype.hasOwnProperty.call(values, key)) result[key] = '0x' + values[key].toString(16);
  }
  return result;
}

function hookValueTypeReader() {
  Interceptor.attach(vaOfRva(RVAS.value_type_reader), {
    onEnter(args) {
      this.objectPtr = args[0];
      this.indexValue = args[1].toInt32();
      this.callerRva = rvaOf(this.returnAddress);
      this.threadId = Process.getCurrentThreadId();
    },
    onLeave(retval) {
      const valueType = retval.toInt32();
      remember(this.threadId, ptrString(this.objectPtr), this.indexValue, {
        value_type: valueType,
        type_caller_rva: this.callerRva,
      });
      bumpCount('value_type_reader');
      if (this.callerRva === '0x331ea2f') {
        writeJson({
          event: 'known_selector_type_return',
          timestamp_ms: nowMs(),
          process_id: processId,
          thread_id: this.threadId,
          caller_rva: this.callerRva,
          enclosing_function: enclosingForCaller(this.callerRva),
          object_ptr: ptrString(this.objectPtr),
          index_value: this.indexValue,
          value_type: valueType,
        });
      }
    },
  });
}

function hookStringLengthReader() {
  Interceptor.attach(vaOfRva(RVAS.string_len_reader), {
    onEnter(args) {
      this.objectPtr = args[0];
      this.indexValue = args[1].toInt32();
      this.callerRva = rvaOf(this.returnAddress);
      this.threadId = Process.getCurrentThreadId();
    },
    onLeave(retval) {
      const length = retval.toInt32();
      remember(this.threadId, ptrString(this.objectPtr), this.indexValue, {
        string_length: length,
        length_caller_rva: this.callerRva,
      });
      bumpCount('string_len_reader');
      if (length === 0x28 || this.callerRva === '0x331eaeb') {
        writeJson({
          event: 'string_length_return',
          timestamp_ms: nowMs(),
          process_id: processId,
          thread_id: this.threadId,
          caller_rva: this.callerRva,
          enclosing_function: enclosingForCaller(this.callerRva),
          object_ptr: ptrString(this.objectPtr),
          index_value: this.indexValue,
          string_length: length,
          length_is_0x28: length === 0x28,
        });
      }
    },
  });
}

function hookStringPointerReader() {
  Interceptor.attach(vaOfRva(RVAS.string_ptr_reader), {
    onEnter(args) {
      this.objectPtr = args[0];
      this.indexValue = args[1].toInt32();
      this.callerRva = rvaOf(this.returnAddress);
      this.threadId = Process.getCurrentThreadId();
      this.callIndex = valueReaderCallIndex++;
    },
    onLeave(retval) {
      const recent = recall(this.threadId, ptrString(this.objectPtr), this.indexValue);
      const pairedLength = recent.string_length;
      const previewLen = pairedLength && pairedLength > 0 && pairedLength <= 512 ? pairedLength : 96;
      const ascii = readAscii(retval, previewLen);
      const flags = idFlags(ascii, pairedLength);
      const callerRva = this.callerRva;
      const enclosing = enclosingForCaller(callerRva);
      bumpCount('string_ptr_reader');
      if (flags.starts_with_extrnlid) {
        bumpCount('extrnlid_value');
        bump(callerCounts, callerRva);
        bump(enclosingCounts, enclosing);
        bump(externalIdCounts, ascii);
      }
      const shouldLog = flags.starts_with_extrnlid || nonExtSamplesWritten < MAX_NON_EXT_SAMPLES;
      if (!shouldLog) return;
      if (!flags.starts_with_extrnlid) nonExtSamplesWritten++;
      const row = {
        event: flags.starts_with_extrnlid ? 'extrnlid_value_reader' : 'string_value_sample',
        value_reader_call_index: this.callIndex,
        timestamp_ms: nowMs(),
        process_id: processId,
        thread_id: this.threadId,
        caller_rva: callerRva,
        return_address: ptrString(this.returnAddress),
        enclosing_function: enclosing,
        first_arg_ptr: ptrString(this.objectPtr),
        index_value: this.indexValue,
        returned_ptr: ptrString(retval),
        returned_ascii_preview: ascii,
        returned_string_length: pairedLength,
        paired_length_caller_rva: recent.length_caller_rva || null,
        paired_value_type: recent.value_type,
        paired_type_caller_rva: recent.type_caller_rva || null,
        first_arg_fields_0x80: flags.starts_with_extrnlid ? safeFields(this.objectPtr, 0x80) : null,
      };
      Object.assign(row, flags);
      if (flags.starts_with_extrnlid) addBacktrace(row, this.context);
      writeJson(row);
    },
  });
}

function hookKnownSelectorSites() {
  for (const [rva, name] of [
    [RVAS.selector_type_call_return, 'selector_type_call_return'],
    [RVAS.selector_len_call_return, 'selector_len_call_return'],
    [RVAS.selector_ptr_call_return, 'selector_ptr_call_return'],
    [RVAS.selector_pre_external_call, 'selector_pre_external_call'],
    [RVAS.selector_post_external_call, 'selector_post_external_call'],
  ]) {
    Interceptor.attach(vaOfRva(rva), {
      onEnter() {
        const len = name === 'selector_len_call_return' ? this.context.rax.toInt32() : null;
        const ascii = name === 'selector_ptr_call_return' ? readAscii(this.context.rax, 40) : null;
        const row = {
          event: name,
          timestamp_ms: nowMs(),
          process_id: processId,
          thread_id: Process.getCurrentThreadId(),
          rva: '0x' + rva.toString(16),
          rax: ptrString(this.context.rax),
          eax: this.context.rax.toInt32(),
          rcx: ptrString(this.context.rcx),
          rdx: ptrString(this.context.rdx),
          r8: ptrString(this.context.r8),
          r9: ptrString(this.context.r9),
          returned_length: len,
          returned_ascii_preview: ascii,
        };
        if (ascii) Object.assign(row, idFlags(ascii, len));
        writeJson(row);
      },
    });
  }
}

function writeSummary(reason) {
  writeJson({
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    extrnlid_caller_histogram: callerCounts,
    extrnlid_enclosing_histogram: enclosingCounts,
    external_id_counts: externalIdCounts,
    non_ext_samples_written: nonExtSamplesWritten,
  });
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  preview_cache_id: PREVIEW_CACHE_ID,
  target_vector_id: TARGET_VECTOR_ID,
  hook_rvas: rvaMapForJson(RVAS),
});

hookValueTypeReader();
hookStringLengthReader();
hookStringPointerReader();
hookKnownSelectorSites();

writeJson({ event: 'ready_hooks_installed', timestamp_ms: nowMs(), process_id: processId, output_path: outPath });

setInterval(function () { writeSummary('periodic'); }, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeSummary('unload');
    out.flush();
    out.close();
  } catch (_) {
  }
});
