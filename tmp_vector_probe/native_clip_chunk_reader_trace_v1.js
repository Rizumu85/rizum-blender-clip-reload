// Read-only Frida trace for CSP chunk-stream reads after .clip file I/O.
// Single native target: CLIPStudioPaint.exe RVA 0x20575A0.

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const TARGET_RVA = 0x20575a0;
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const MAX_BACKTRACES = 60;
const MAX_PREFIX_BYTES = 96;
const MAX_EVENTS = 2000;
const SUMMARY_INTERVAL_MS = 5000;
const INCLUDED_CALLER_RVAS = {
  '0x3a40523': 'main_magic_csfchunk',
  '0x3a4065d': 'main_chunk_sqlite_or_header',
  '0x3a40782': 'main_chunk_body',
  '0x3a41086': 'header_sniff_csfchunk',
  '0x3a41cd1': 'external_chunk_magic',
  '0x3a41d7f': 'external_chunk_body',
  '0x3a41fb6': 'load_probe_csfchunk',
};

const cspBase = Process.getModuleByName(CSP_MODULE).base;
const target = cspBase.add(TARGET_RVA);
const processId = Process.id;
const outPath = `${OUT_DIR}/native_clip_chunk_reader_trace_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

const counts = {};
const callerCounts = {};
const signatureCounts = {};
let eventIndex = 0;
let skippedEventIndex = 0;
let backtracesWritten = 0;

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

function readPrefix(buffer, length) {
  const n = Math.min(length || 0, MAX_PREFIX_BYTES);
  if (!buffer || n <= 0) return { hex: null, ascii: null, signature: null };
  try {
    const bytes = buffer.readByteArray(n);
    const u8 = new Uint8Array(bytes);
    const hex = Array.from(u8).map((b) => b.toString(16).padStart(2, '0')).join(' ');
    const ascii = Array.from(u8).map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.')).join('');
    return { hex, ascii, signature: classifySignature(u8, ascii) };
  } catch (_) {
    return { hex: null, ascii: null, signature: 'unreadable' };
  }
}

function classifySignature(u8, ascii) {
  if (ascii.startsWith('CSFCHUNK')) return 'CSFCHUNK';
  if (ascii.startsWith('CHNKHead')) return 'CHNKHead';
  if (ascii.startsWith('CHNKSQLi')) return 'CHNKSQLi';
  if (ascii.startsWith('SQLite format 3')) return 'SQLite';
  if (ascii.startsWith('CHNKExta')) return 'CHNKExta';
  if (ascii.includes('extrnlid')) return 'extrnlid';
  if (u8.length >= 8) return ascii.slice(0, 8);
  if (u8.length) return ascii;
  return 'empty';
}

function bump(callerRva, signature) {
  counts.chunk_read = (counts.chunk_read || 0) + 1;
  const caller = callerRva || 'unknown';
  callerCounts[caller] = (callerCounts[caller] || 0) + 1;
  const sig = signature || 'unknown';
  signatureCounts[sig] = (signatureCounts[sig] || 0) + 1;
}

writeJson({
  event: 'ready_start',
  output_path: outPath,
  process_id: processId,
  csp_module_base: cspBase.toString(),
  target_rva: `0x${TARGET_RVA.toString(16)}`,
  target_va: target.toString(),
});

Interceptor.attach(target, {
  onEnter(args) {
    this.stream = args[0];
    this.dest = args[1];
    this.requested = args[2].toUInt32();
    this.caller = this.returnAddress;
    this.callerRva = rvaOf(this.returnAddress);
    this.routeLabel = INCLUDED_CALLER_RVAS[this.callerRva] || null;
    this.index = this.routeLabel ? eventIndex++ : skippedEventIndex++;
  },
  onLeave(retval) {
    if (!this.routeLabel) return;
    const returned = retval.toInt32();
    const prefix = readPrefix(this.dest, Math.min(this.requested, returned > 0 ? returned : this.requested));
    bump(this.callerRva, prefix.signature);
    if (this.index >= MAX_EVENTS) return;
    const row = {
      event: 'chunk_read',
      chunk_read_index: this.index,
      timestamp_ms: nowMs(),
      thread_id: Process.getCurrentThreadId(),
      process_id: processId,
      caller: ptrString(this.caller),
      caller_rva: this.callerRva,
      route_label: this.routeLabel,
      stream_ptr: ptrString(this.stream),
      dest_ptr: ptrString(this.dest),
      requested_size: this.requested,
      return_value: returned,
      buffer_prefix_hex: prefix.hex,
      buffer_prefix_ascii: prefix.ascii,
      signature: prefix.signature,
    };
    addBacktrace(row, this.context);
    writeJson(row);
  },
});

writeJson({
  event: 'ready_hooks_installed',
  output_path: outPath,
  process_id: processId,
  csp_module_base: cspBase.toString(),
  target_rva: `0x${TARGET_RVA.toString(16)}`,
  included_caller_rvas: INCLUDED_CALLER_RVAS,
});

setInterval(function () {
  writeJson({
    event: 'summary',
    reason: 'periodic',
    timestamp_ms: nowMs(),
    counts,
    caller_counts: callerCounts,
    signature_counts: signatureCounts,
    skipped_non_route_events: skippedEventIndex,
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
      signature_counts: signatureCounts,
      skipped_non_route_events: skippedEventIndex,
    });
    out.flush();
    out.close();
  } catch (_) {
  }
});
