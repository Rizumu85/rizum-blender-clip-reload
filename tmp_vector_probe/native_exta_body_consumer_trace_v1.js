// Read-only trace for CSP CHNKExta external body consumption.
//
// Target observation:
//   0x143A41D7A calls 0x1420575A0 to read the CHNKExta body payload.
//   0x143A41D7F is the return address after that read, not a renderer.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';

const READER_RVA = 0x20575a0;
const SCALAR_RVA = 0x2057b70;

const CALLERS = {
  0x3a41cd1: 'external_magic_read',
  0x3a41d02: 'external_unknown_scalar_after_magic',
  0x3a41d0e: 'external_id_size_scalar',
  0x3a41d2f: 'external_id_read',
  0x3a41d53: 'external_body_size_scalar',
  0x3a41d7f: 'external_body_read',
};

const MAX_PREFIX_BYTES = 256;
const MAX_BACKTRACES = 30;
const SUMMARY_INTERVAL_MS = 5000;

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_exta_body_consumer_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

let extBodyIndex = 0;
let readEventIndex = 0;
let scalarEventIndex = 0;
let backtracesWritten = 0;
const counts = {};
const callerCounts = {};
const sizeCounts = {};
const signatureCounts = {};
const pendingByThread = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() {
  return Date.now();
}

function tidKey() {
  return String(Process.getCurrentThreadId());
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

function callerRvaNumber(p) {
  try {
    return p.sub(cspBase).toUInt32();
  } catch (_) {
    return -1;
  }
}

function writeJson(row) {
  out.write(JSON.stringify(row) + '\n');
  out.flush();
}

function bump(obj, key) {
  const k = key === null || key === undefined ? 'unknown' : String(key);
  obj[k] = (obj[k] || 0) + 1;
}

function pending() {
  const key = tidKey();
  if (!pendingByThread[key]) pendingByThread[key] = {};
  return pendingByThread[key];
}

function addBacktrace(row, context) {
  if (backtracesWritten >= MAX_BACKTRACES) return;
  backtracesWritten++;
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 20)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
}

function readBytes(ptrValue, length) {
  if (!ptrValue || length <= 0) return null;
  try {
    return new Uint8Array(ptrValue.readByteArray(length));
  } catch (_) {
    return null;
  }
}

function toHex(u8, maxBytes) {
  if (!u8) return null;
  const n = Math.min(u8.length, maxBytes || u8.length);
  return Array.from(u8.slice(0, n)).map((b) => b.toString(16).padStart(2, '0')).join(' ');
}

function toAscii(u8, maxBytes) {
  if (!u8) return null;
  const n = Math.min(u8.length, maxBytes || u8.length);
  return Array.from(u8.slice(0, n)).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
}

function bytesContainAscii(u8, s) {
  if (!u8) return false;
  const needle = Array.from(s).map((ch) => ch.charCodeAt(0));
  outer:
  for (let i = 0; i <= u8.length - needle.length; i++) {
    for (let j = 0; j < needle.length; j++) {
      if (u8[i + j] !== needle[j]) continue outer;
    }
    return true;
  }
  return false;
}

function bytesContainUtf16Be(u8, s) {
  if (!u8) return false;
  const needle = [];
  for (const ch of s) {
    needle.push(0, ch.charCodeAt(0));
  }
  outer:
  for (let i = 0; i <= u8.length - needle.length; i++) {
    for (let j = 0; j < needle.length; j++) {
      if (u8[i + j] !== needle[j]) continue outer;
    }
    return true;
  }
  return false;
}

function classify(u8) {
  if (!u8 || u8.length === 0) return 'empty';
  const ascii = toAscii(u8, Math.min(u8.length, 64)) || '';
  if (ascii.startsWith('CHNKExta')) return 'CHNKExta';
  if (ascii.startsWith('CSFCHUNK')) return 'CSFCHUNK';
  if (ascii.startsWith('SQLite format 3')) return 'SQLite';
  if (ascii.includes('extrnlid')) return 'extrnlid';
  if (bytesContainUtf16Be(u8, 'BlockDataBeginChunk')) return 'BlockDataBeginChunk';
  if (u8.length >= 2 && u8[0] === 0x78) return 'zlib';
  if (u8.length >= 6 && u8[4] === 0x78) return 'size_prefixed_zlib';
  return ascii.slice(0, 16);
}

function preview(ptrValue, requested) {
  const n = Math.min(Number(requested) || 0, MAX_PREFIX_BYTES);
  const u8 = readBytes(ptrValue, n);
  const signature = classify(u8);
  return {
    hex: toHex(u8, n),
    ascii: toAscii(u8, n),
    signature,
    contains_block_data_begin_chunk:
      bytesContainAscii(u8, 'BlockDataBeginChunk') || bytesContainUtf16Be(u8, 'BlockDataBeginChunk'),
  };
}

function asciiIdFromBytes(u8) {
  if (!u8) return null;
  let s = '';
  for (const b of u8) {
    s += b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.';
  }
  return s;
}

function makeSummary(reason) {
  const pendingDepths = {};
  for (const key of Object.keys(pendingByThread)) {
    pendingDepths[key] = Object.keys(pendingByThread[key]).length;
  }
  return {
    event: 'summary',
    reason,
    timestamp_ms: nowMs(),
    process_id: processId,
    output_path: outPath,
    counts,
    caller_counts: callerCounts,
    requested_size_counts: sizeCounts,
    signature_counts: signatureCounts,
    pending_threads: pendingDepths,
  };
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_path: csp.path,
  module_base: cspBase.toString(),
  output_path: outPath,
  hooks: {
    reader_rva: '0x' + READER_RVA.toString(16),
    scalar_rva: '0x' + SCALAR_RVA.toString(16),
    observed_return_rvas: Object.fromEntries(Object.entries(CALLERS).map(([k, v]) => ['0x' + Number(k).toString(16), v])),
  },
});

Interceptor.attach(cspBase.add(READER_RVA), {
  onEnter(args) {
    this.stream = args[0];
    this.dest = args[1];
    this.requested = args[2].toUInt32();
    this.returnAddr = this.returnAddress;
    this.callerNum = callerRvaNumber(this.returnAddress);
    this.route = CALLERS[this.callerNum] || null;
  },
  onLeave(retval) {
    if (!this.route) return;
    bump(counts, 'reader_call');
    bump(callerCounts, rvaOf(this.returnAddr));
    bump(sizeCounts, this.requested);

    const p = pending();
    if (this.route === 'external_magic_read') {
      p.magic = preview(this.dest, this.requested);
      p.magic_stream = ptrString(this.stream);
      p.magic_dest = ptrString(this.dest);
      p.started_ms = nowMs();
    } else if (this.route === 'external_id_read') {
      const idBytes = readBytes(this.dest, Math.min(this.requested, 0x28));
      p.external_id_raw_hex = toHex(idBytes, 0x28);
      p.external_id_ascii = asciiIdFromBytes(idBytes);
      p.external_id_dest = ptrString(this.dest);
    }

    const prefix = preview(this.dest, this.requested);
    bump(signatureCounts, prefix.signature);

    const row = {
      event: 'exta_reader_return',
      read_event_index: readEventIndex++,
      timestamp_ms: nowMs(),
      process_id: processId,
      thread_id: Process.getCurrentThreadId(),
      route: this.route,
      caller_rva: rvaOf(this.returnAddr),
      caller_va: ptrString(this.returnAddr),
      stream_ptr: ptrString(this.stream),
      dest_ptr: ptrString(this.dest),
      requested_size: this.requested,
      return_value_raw: retval.toString(),
      prefix_hex: prefix.hex,
      prefix_ascii: prefix.ascii,
      signature: prefix.signature,
      contains_block_data_begin_chunk: prefix.contains_block_data_begin_chunk,
    };

    if (this.route === 'external_body_read') {
      row.event = 'external_body';
      row.ext_body_index = extBodyIndex++;
      row.external_id_raw_hex = p.external_id_raw_hex || null;
      row.external_id_ascii = p.external_id_ascii || null;
      row.magic_signature = p.magic ? p.magic.signature : null;
      row.magic_prefix_ascii = p.magic ? p.magic.ascii : null;
      row.nearby_external_id = p.external_id_ascii || null;
      row.pointer_stored_after_read = null;
      row.pointer_store_status = 'not_identified_in_static_audit';
      row.post_read_static_calls = [
        { rva: '0x3a41d80', target_rva: '0x20493f0', role: 'cleanup_stack_object_rsp_c0' },
        { rva: '0x3a41d8e', target_rva: '0x20493f0', role: 'cleanup_stack_object_rsp_f0' },
      ];
      addBacktrace(row, this.context);
      delete pendingByThread[tidKey()];
    }
    writeJson(row);
  },
});

Interceptor.attach(cspBase.add(SCALAR_RVA), {
  onEnter(args) {
    this.returnAddr = this.returnAddress;
    this.callerNum = callerRvaNumber(this.returnAddress);
    this.route = CALLERS[this.callerNum] || null;
    this.stream = args[0];
  },
  onLeave(retval) {
    if (!this.route) return;
    bump(counts, 'scalar_call');
    bump(callerCounts, rvaOf(this.returnAddr));
    const value = retval.toString();
    const p = pending();
    if (this.route === 'external_unknown_scalar_after_magic') {
      p.scalar_after_magic = value;
    } else if (this.route === 'external_id_size_scalar') {
      p.external_id_size = value;
    } else if (this.route === 'external_body_size_scalar') {
      p.external_body_size = value;
    }
    writeJson({
      event: 'exta_scalar_return',
      scalar_event_index: scalarEventIndex++,
      timestamp_ms: nowMs(),
      process_id: processId,
      thread_id: Process.getCurrentThreadId(),
      route: this.route,
      caller_rva: rvaOf(this.returnAddr),
      caller_va: ptrString(this.returnAddr),
      stream_ptr: ptrString(this.stream),
      return_value_raw: value,
    });
  },
});

writeJson({
  event: 'ready_hooks_installed',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  reader_va: cspBase.add(READER_RVA).toString(),
  scalar_va: cspBase.add(SCALAR_RVA).toString(),
  output_path: outPath,
});

setInterval(function () {
  writeJson(makeSummary('periodic'));
}, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeJson(makeSummary('unload'));
    out.flush();
    out.close();
  } catch (_) {
  }
});
