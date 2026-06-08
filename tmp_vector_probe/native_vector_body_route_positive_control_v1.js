// Read-only positive-control trace for Vector_SizePressure body-load route.
//
// One run should answer whether the attached CSP process reaches:
//   A. Win32 file read of the target .clip
//   B. shared chunk reader 0x1420575A0
//   C. external body payload caller 0x3A41D7F
//   D. external body loader 0x143A41780
//   E. registration caller 0x143A3E180

'use strict';

const CSP_MODULE = 'CLIPStudioPaint.exe';
const OUT_DIR = 'E:/Documents/Claude/Projects/rizum-blender-clip-reload/tmp_vector_probe';
const TARGET_NEEDLES = ['vector_sizepressure', 'vector_sizepressure_route_positive'];
const TARGET_EXT_ID = 'extrnlid62D15CB4395245648869B4AEBAD8FBCE';
const TARGET_BODY_SIZE = 2644;
const TARGET_HASH = 'fnv1a32:7bece4ac';
const PREFIX_BYTES = 96;
const HASH_BYTES = 4096;
const MAX_FILE_BACKTRACES = 20;
const MAX_ROUTE_BACKTRACES = 40;
const SUMMARY_INTERVAL_MS = 5000;

const RVAS = {
  chunk_reader: 0x20575a0,
  loader_entry: 0x3a41780,
  loader_post_body_size: 0x3a41d58,
  loader_post_allocation: 0x3a41d6d,
  loader_post_body_read: 0x3a41d7f,
  registration_entry: 0x3a3e180,
  registration_first_pre_call: 0x3a3e1a7,
  registration_first_post_call: 0x3a3e1ac,
  registration_second_pre_call: 0x3a3e1d1,
  registration_second_post_call: 0x3a3e1d6,
  registration_success_flag_write: 0x3a3e1b0,
};

const CHUNK_ROUTE_CALLERS = {
  '0x3a40523': 'main_magic_csfchunk',
  '0x3a4065d': 'main_chunk_sqlite_or_header',
  '0x3a40782': 'main_chunk_body',
  '0x3a41cd1': 'external_chunk_magic',
  '0x3a41d7f': 'external_chunk_body',
  '0x3a41086': 'header_sniff_csfchunk',
  '0x3a41fb6': 'load_probe_csfchunk',
};

const csp = Process.getModuleByName(CSP_MODULE);
const cspBase = csp.base;
const processId = Process.id;
const outPath = `${OUT_DIR}/native_vector_body_route_positive_control_${makeTimestamp()}_pid${processId}.jsonl`;
const out = new File(outPath, 'w');

const handleToState = {};
const counts = {};
const fileCallerCounts = {};
const chunkCallerCounts = {};
const signatureCounts = {};
const style = {};
let fileBacktracesWritten = 0;
let routeBacktracesWritten = 0;
let readFileCallIndex = 0;
let targetReadFileCallIndex = 0;
let chunkReadIndex = 0;
let loaderInvocationIndex = 0;
let registrationInvocationIndex = 0;

const loaderStackByThread = {};
const registrationStackByThread = {};

function makeTimestamp() {
  const d = new Date();
  const pad = (n, w = 2) => String(n).padStart(w, '0');
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}_${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}_${pad(d.getMilliseconds(), 3)}`;
}

function nowMs() { return Date.now(); }
function tidKey() { return String(Process.getCurrentThreadId()); }
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

function lower(s) { return String(s || '').toLowerCase(); }

function isTargetPath(path) {
  const s = lower(path);
  return s.endsWith('.clip') && TARGET_NEEDLES.some((needle) => s.includes(needle));
}

function readUtf16(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.readUtf16String();
  } catch (_) {
    return null;
  }
}

function readU32Ptr(p) {
  try {
    if (p === null || p.isNull()) return null;
    return p.readU32();
  } catch (_) {
    return null;
  }
}

function safeReadU32(p) {
  try { return p.readU32(); } catch (_) { return null; }
}

function safeReadPointer(p) {
  try { return p.readPointer(); } catch (_) { return null; }
}

function stack(map) {
  const k = tidKey();
  if (!map[k]) map[k] = [];
  return map[k];
}

function activeLoader() {
  const s = stack(loaderStackByThread);
  return s.length ? s[s.length - 1] : null;
}

function activeRegistration() {
  const s = stack(registrationStackByThread);
  return s.length ? s[s.length - 1] : null;
}

function isInvalidHandle(h) {
  try {
    return h.isNull() || h.toString().toLowerCase() === '0xffffffffffffffff';
  } catch (_) {
    return true;
  }
}

function handleKey(h) { return ptrString(h); }

function toSignedNumber(p) {
  try {
    const v = BigInt(p.toString());
    const signed = v > 0x7fffffffffffffffn ? v - 0x10000000000000000n : v;
    if (signed >= BigInt(Number.MIN_SAFE_INTEGER) && signed <= BigInt(Number.MAX_SAFE_INTEGER)) return Number(signed);
  } catch (_) {
  }
  return null;
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

function readAscii(ptrValue, length) {
  return toAscii(readBytes(ptrValue, length), length);
}

function checksum(u8) {
  if (!u8) return null;
  let h = 0x811c9dc5;
  for (const b of u8) {
    h ^= b;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return 'fnv1a32:' + h.toString(16).padStart(8, '0');
}

function classifySignature(u8, ascii) {
  if (!u8) return 'unreadable';
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

function readPrefix(ptrValue, length) {
  const n = Math.min(length || 0, PREFIX_BYTES);
  if (!ptrValue || n <= 0) return { hex: null, ascii: null, signature: null, probable_type: null };
  const u8 = readBytes(ptrValue, n);
  if (!u8) return { hex: null, ascii: null, signature: 'unreadable', probable_type: 'unreadable' };
  const ascii = toAscii(u8, n);
  const signature = classifySignature(u8, ascii);
  return {
    hex: toHex(u8, n),
    ascii,
    signature,
    probable_type: signature,
  };
}

function addBacktrace(row, context, kind) {
  if (kind === 'file') {
    if (fileBacktracesWritten >= MAX_FILE_BACKTRACES) return;
    fileBacktracesWritten++;
  } else {
    if (routeBacktracesWritten >= MAX_ROUTE_BACKTRACES) return;
    routeBacktracesWritten++;
  }
  try {
    row.backtrace = Thread.backtrace(context, Backtracer.ACCURATE)
      .slice(0, 18)
      .map((p) => ({ va: p.toString(), rva: rvaOf(p), symbol: DebugSymbol.fromAddress(p).toString() }));
  } catch (_) {
    row.backtrace = [];
  }
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

function hookExport(moduleNames, exportName, callbacks) {
  for (const moduleName of moduleNames) {
    const p = findExport(moduleName, exportName);
    if (!p) continue;
    Interceptor.attach(p, callbacks(exportName, moduleName, p));
    writeJson({ event: 'hook_installed', kind: 'api', function: exportName, module: moduleName, address: p.toString() });
    return true;
  }
  writeJson({ event: 'hook_missing', kind: 'api', function: exportName, modules: moduleNames });
  return false;
}

function fileCallerRva(context) {
  const caller = context.returnAddress || NULL;
  return rvaOf(caller);
}

function hookFileApi() {
  const modules = ['KernelBase.dll', 'kernel32.dll'];
  hookExport(modules, 'CreateFileW', function (functionName, moduleName) {
    return {
      onEnter(args) {
        this.path = readUtf16(args[0]);
        this.isTarget = isTargetPath(this.path);
        this.desiredAccess = args[1].toString();
        this.shareMode = args[2].toString();
        this.creationDisposition = args[4].toString();
        this.callerRva = fileCallerRva(this.context);
      },
      onLeave(retval) {
        if (!this.isTarget) return;
        bumpCount('file_target_createfile');
        style.file_target_seen = true;
        if (!isInvalidHandle(retval)) {
          handleToState[handleKey(retval)] = { path: this.path, offset: 0, handle: ptrString(retval) };
        }
        const row = {
          event: 'file_api',
          function: functionName,
          timestamp_ms: nowMs(),
          thread_id: Process.getCurrentThreadId(),
          process_id: processId,
          path: this.path,
          handle: ptrString(retval),
          desired_access: this.desiredAccess,
          share_mode: this.shareMode,
          creation_disposition: this.creationDisposition,
          caller_rva: this.callerRva,
          return_value: ptrString(retval),
        };
        writeJson(row);
      },
    };
  });

  hookExport(modules, 'ReadFile', function (functionName) {
    return {
      onEnter(args) {
        this.callIndex = readFileCallIndex++;
        this.handle = args[0];
        this.buffer = args[1];
        this.requested = args[2].toUInt32();
        this.bytesReadPtr = args[3];
        this.overlapped = args[4];
        this.handleState = handleToState[handleKey(this.handle)] || null;
        this.isTarget = Boolean(this.handleState);
        this.offsetBefore = this.handleState ? this.handleState.offset : null;
        this.path = this.handleState ? this.handleState.path : null;
        this.callerRva = fileCallerRva(this.context);
      },
      onLeave(retval) {
        if (!this.isTarget) return;
        const bytesRead = readU32Ptr(this.bytesReadPtr);
        const prefix = readPrefix(this.buffer, bytesRead || this.requested);
        const key = handleKey(this.handle);
        if (handleToState[key] && bytesRead !== null) handleToState[key].offset += bytesRead;
        bumpCount('file_target_readfile');
        bump(fileCallerCounts, this.callerRva);
        const row = {
          event: 'file_read',
          read_call_index: this.callIndex,
          target_read_index: targetReadFileCallIndex++,
          timestamp_ms: nowMs(),
          thread_id: Process.getCurrentThreadId(),
          process_id: processId,
          path: this.path,
          handle: ptrString(this.handle),
          offset: this.offsetBefore,
          requested_size: this.requested,
          bytes_read: bytesRead,
          prefix_ascii: prefix.ascii,
          prefix_hex: prefix.hex,
          probable_type: prefix.probable_type,
          caller_rva: this.callerRva,
          return_value: retval.toInt32(),
        };
        addBacktrace(row, this.context, 'file');
        writeJson(row);
      },
    };
  });

  hookExport(modules, 'SetFilePointerEx', function (functionName) {
    return {
      onEnter(args) {
        this.handle = args[0];
        this.distance = toSignedNumber(args[1]);
        this.newFilePointer = args[2];
        this.moveMethod = args[3].toInt32();
        this.handleState = handleToState[handleKey(this.handle)] || null;
        this.isTarget = Boolean(this.handleState);
        this.callerRva = fileCallerRva(this.context);
      },
      onLeave(retval) {
        if (!this.isTarget) return;
        let after = null;
        if (this.moveMethod === 0) after = this.distance;
        else if (this.moveMethod === 1) after = (this.handleState.offset || 0) + (this.distance || 0);
        if (after !== null) this.handleState.offset = after;
        bumpCount('file_target_setpointer');
        writeJson({
          event: 'file_api',
          function: functionName,
          timestamp_ms: nowMs(),
          thread_id: Process.getCurrentThreadId(),
          process_id: processId,
          path: this.handleState.path,
          handle: ptrString(this.handle),
          offset: this.handleState.offset,
          distance: this.distance,
          move_method: this.moveMethod,
          caller_rva: this.callerRva,
          return_value: retval.toInt32(),
        });
      },
    };
  });

  hookExport(modules, 'CloseHandle', function (functionName) {
    return {
      onEnter(args) {
        this.handle = args[0];
        this.handleState = handleToState[handleKey(this.handle)] || null;
        this.isTarget = Boolean(this.handleState);
        this.callerRva = fileCallerRva(this.context);
      },
      onLeave(retval) {
        if (!this.isTarget) return;
        bumpCount('file_target_closehandle');
        writeJson({
          event: 'file_api',
          function: functionName,
          timestamp_ms: nowMs(),
          thread_id: Process.getCurrentThreadId(),
          process_id: processId,
          path: this.handleState.path,
          handle: ptrString(this.handle),
          offset: this.handleState.offset,
          caller_rva: this.callerRva,
          return_value: retval.toInt32(),
        });
        delete handleToState[handleKey(this.handle)];
      },
    };
  });
}

function hookChunkReader() {
  Interceptor.attach(vaOfRva(RVAS.chunk_reader), {
    onEnter(args) {
      this.stream = args[0];
      this.dest = args[1];
      this.requested = args[2].toUInt32();
      this.caller = this.returnAddress;
      this.callerRva = rvaOf(this.returnAddress);
      this.routeLabel = CHUNK_ROUTE_CALLERS[this.callerRva] || null;
      this.index = chunkReadIndex++;
    },
    onLeave(retval) {
      if (!this.routeLabel) return;
      const returned = retval.toInt32();
      const prefix = readPrefix(this.dest, Math.min(this.requested, returned > 0 ? returned : this.requested));
      bumpCount('chunk_reader_route_call');
      bump(chunkCallerCounts, this.callerRva);
      bump(signatureCounts, prefix.signature);
      if (this.callerRva === '0x3a41d7f') style.saw_chunk_reader_external_body = true;
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
        buffer_prefix_ascii: prefix.ascii,
        buffer_prefix_hex: prefix.hex,
        signature: prefix.signature,
      };
      addBacktrace(row, this.context, 'route');
      writeJson(row);
    },
  });
}

function externalIdMatches(rec) {
  return rec && rec.external_id_ascii === TARGET_EXT_ID;
}

function bodySizeMatches(rec) {
  return rec && rec.body_size === TARGET_BODY_SIZE;
}

function hashMatches(rec) {
  return rec && rec.dest_hash === TARGET_HASH;
}

function hookExternalBodyLoader() {
  Interceptor.attach(vaOfRva(RVAS.loader_entry), {
    onEnter(args) {
      const rec = {
        invocation_index: loaderInvocationIndex++,
        thread_id: Process.getCurrentThreadId(),
        caller_rva: rvaOf(this.returnAddress),
        return_address: ptrString(this.returnAddress),
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: args[3].toUInt32(),
        external_id_length: args[3].toUInt32(),
        external_id_ascii: readAscii(args[2], args[3].toUInt32()),
        body_size: null,
        dest_ptr: null,
        dest_hash: null,
        prefix_ascii: null,
        prefix_hex: null,
      };
      stack(loaderStackByThread).push(rec);
      bumpCount('loader_invocation');
      if (externalIdMatches(rec)) bumpCount('loader_vector_external_id_match');
      writeJson(Object.assign({ event: 'loader_entry', timestamp_ms: nowMs(), process_id: processId }, rec));
    },
    onLeave(retval) {
      const s = stack(loaderStackByThread);
      const rec = s.length ? s.pop() : null;
      if (!rec) {
        writeJson({ event: 'loader_leave_unpaired', timestamp_ms: nowMs(), process_id: processId, return_value: retval.toInt32() });
        return;
      }
      rec.return_value = retval.toInt32();
      rec.matches_target_external_id = externalIdMatches(rec);
      rec.matches_body_size_2644 = bodySizeMatches(rec);
      rec.matches_hash_7bece4ac = hashMatches(rec);
      if (bodySizeMatches(rec)) bumpCount('loader_body_size_2644_match');
      if (hashMatches(rec)) bumpCount('loader_hash_7bece4ac_match');
      writeJson(Object.assign({ event: 'loader_leave', timestamp_ms: nowMs(), process_id: processId }, rec));
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_body_size), {
    onEnter() {
      const rec = activeLoader();
      if (!rec) return;
      rec.body_size = this.context.rax.toUInt32();
      if (rec.body_size === TARGET_BODY_SIZE) bumpCount('loader_body_size_2644_observed');
      writeJson({
        event: 'loader_post_body_size',
        timestamp_ms: nowMs(),
        process_id: processId,
        invocation_index: rec.invocation_index,
        body_size: rec.body_size,
        external_id_ascii: rec.external_id_ascii,
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_allocation), {
    onEnter() {
      const rec = activeLoader();
      if (!rec) return;
      rec.dest_ptr = ptrString(this.context.rax);
      writeJson({
        event: 'loader_post_allocation',
        timestamp_ms: nowMs(),
        process_id: processId,
        invocation_index: rec.invocation_index,
        dest_ptr: rec.dest_ptr,
        body_size: rec.body_size,
        external_id_ascii: rec.external_id_ascii,
      });
    },
  });

  Interceptor.attach(vaOfRva(RVAS.loader_post_body_read), {
    onEnter() {
      const rec = activeLoader();
      if (!rec || !rec.dest_ptr) return;
      const dest = ptr(rec.dest_ptr);
      const prefix = readPrefix(dest, Math.min(PREFIX_BYTES, rec.body_size || PREFIX_BYTES));
      const hashBytes = readBytes(dest, Math.min(HASH_BYTES, rec.body_size || HASH_BYTES));
      rec.prefix_ascii = prefix.ascii;
      rec.prefix_hex = prefix.hex;
      rec.dest_hash = checksum(hashBytes);
      if (hashMatches(rec)) bumpCount('loader_hash_7bece4ac_observed');
      writeJson({
        event: 'loader_post_body_read',
        timestamp_ms: nowMs(),
        process_id: processId,
        invocation_index: rec.invocation_index,
        external_id_ascii: rec.external_id_ascii,
        body_size: rec.body_size,
        dest_ptr: rec.dest_ptr,
        prefix_ascii: rec.prefix_ascii,
        prefix_hex: rec.prefix_hex,
        dest_hash: rec.dest_hash,
        matches_target_external_id: externalIdMatches(rec),
        matches_body_size_2644: bodySizeMatches(rec),
        matches_hash_7bece4ac: hashMatches(rec),
      });
    },
  });

  for (const rva of [RVAS.registration_first_post_call, RVAS.registration_second_post_call]) {
    Interceptor.attach(vaOfRva(rva), {
      onEnter() {
        const rec = activeLoader();
        writeJson({
          event: 'loader_caller_post_return',
          timestamp_ms: nowMs(),
          process_id: processId,
          caller_site_rva: '0x' + rva.toString(16),
          invocation_index: rec ? rec.invocation_index : null,
          eax: this.context.rax.toInt32(),
          external_id_ascii: rec ? rec.external_id_ascii : null,
          body_size: rec ? rec.body_size : null,
          dest_ptr: rec ? rec.dest_ptr : null,
          dest_hash: rec ? rec.dest_hash : null,
        });
      },
    });
  }
}

function snapshotFields(p) {
  if (!p || p.isNull()) return null;
  const rows = {};
  for (const off of [0x100, 0x250, 0x3f4]) {
    rows['0x' + off.toString(16)] = {
      ptr: ptrString(safeReadPointer(p.add(off))),
      u32: safeReadU32(p.add(off)),
    };
  }
  return rows;
}

function hookRegistrationCaller() {
  Interceptor.attach(vaOfRva(RVAS.registration_entry), {
    onEnter(args) {
      const rec = {
        parent_invocation_index: registrationInvocationIndex++,
        thread_id: Process.getCurrentThreadId(),
        caller_rva: rvaOf(this.returnAddress),
        return_address: ptrString(this.returnAddress),
        rcx: ptrString(args[0]),
        rdx: ptrString(args[1]),
        r8: ptrString(args[2]),
        r9: args[3].toUInt32(),
        external_id_ascii: readAscii(args[2], args[3].toUInt32()),
        external_id_length: args[3].toUInt32(),
        parent_ptr: ptrString(args[0]),
        candidate_owner_slot_0x100: ptrString(args[0].add(0x100)),
        candidate_owner_slot_0x250: ptrString(args[0].add(0x250)),
        fields_before: snapshotFields(args[0]),
      };
      stack(registrationStackByThread).push(rec);
      bumpCount('registration_invocation');
      writeJson(Object.assign({ event: 'registration_entry', timestamp_ms: nowMs(), process_id: processId }, rec));
    },
    onLeave(retval) {
      const s = stack(registrationStackByThread);
      const rec = s.length ? s.pop() : null;
      writeJson({
        event: 'registration_leave',
        timestamp_ms: nowMs(),
        process_id: processId,
        parent_invocation_index: rec ? rec.parent_invocation_index : null,
        parent_ptr: rec ? rec.parent_ptr : null,
        return_value: retval.toInt32(),
        fields_after: rec && rec.parent_ptr ? snapshotFields(ptr(rec.parent_ptr)) : null,
      });
    },
  });

  const simpleSites = [
    [RVAS.registration_first_pre_call, 'registration_first_pre_call'],
    [RVAS.registration_first_post_call, 'registration_first_post_call'],
    [RVAS.registration_second_pre_call, 'registration_second_pre_call'],
    [RVAS.registration_second_post_call, 'registration_second_post_call'],
    [RVAS.registration_success_flag_write, 'registration_success_flag_write'],
  ];
  for (const [rva, name] of simpleSites) {
    Interceptor.attach(vaOfRva(rva), {
      onEnter() {
        const rec = activeRegistration();
        writeJson({
          event: name,
          timestamp_ms: nowMs(),
          process_id: processId,
          parent_invocation_index: rec ? rec.parent_invocation_index : null,
          parent_ptr: rec ? rec.parent_ptr : null,
          rbx: ptrString(this.context.rbx),
          r12: ptrString(this.context.r12),
          r13: ptrString(this.context.r13),
          eax: this.context.rax.toInt32(),
          external_id_ascii: rec ? rec.external_id_ascii : null,
          external_id_length: rec ? rec.external_id_length : null,
          selected_fields: rec && rec.parent_ptr ? snapshotFields(ptr(rec.parent_ptr)) : null,
        });
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
    file_target_seen: Boolean(style.file_target_seen),
    total_target_readfile_calls: counts.file_target_readfile || 0,
    total_chunk_reader_route_calls: counts.chunk_reader_route_call || 0,
    chunk_reader_caller_histogram: chunkCallerCounts,
    file_read_caller_histogram: fileCallerCounts,
    signature_histogram: signatureCounts,
    total_0x143a41780_invocations: counts.loader_invocation || 0,
    total_vector_external_id_matches: counts.loader_vector_external_id_match || 0,
    total_body_size_2644_matches: (counts.loader_body_size_2644_match || 0) + (counts.loader_body_size_2644_observed || 0),
    total_fnv1a32_7bece4ac_matches: (counts.loader_hash_7bece4ac_match || 0) + (counts.loader_hash_7bece4ac_observed || 0),
    total_0x143a3e180_invocations: counts.registration_invocation || 0,
    did_0x20575a0_caller_0x3a41d7f_occur: Boolean(style.saw_chunk_reader_external_body),
    did_0x143a41780_occur: Boolean(counts.loader_invocation),
    did_0x143a3e180_occur: Boolean(counts.registration_invocation),
    counts,
  });
}

writeJson({
  event: 'ready',
  timestamp_ms: nowMs(),
  process_id: processId,
  module_base: cspBase.toString(),
  module_path: csp.path,
  output_path: outPath,
  target_ext_id: TARGET_EXT_ID,
  target_body_size: TARGET_BODY_SIZE,
  target_hash: TARGET_HASH,
  hook_rvas: Object.fromEntries(Object.entries(RVAS).map(([k, v]) => [k, '0x' + v.toString(16)])),
});

hookFileApi();
hookChunkReader();
hookExternalBodyLoader();
hookRegistrationCaller();

writeJson({
  event: 'ready_hooks_installed',
  timestamp_ms: nowMs(),
  process_id: processId,
  output_path: outPath,
  chunk_route_callers: CHUNK_ROUTE_CALLERS,
});

setInterval(function () { writeSummary('periodic'); }, SUMMARY_INTERVAL_MS);

Script.bindWeak(out, function () {
  try {
    writeSummary('unload');
    out.flush();
    out.close();
  } catch (_) {
  }
});
