let wasm;

const cachedTextDecoder = (typeof TextDecoder !== 'undefined' ? new TextDecoder('utf-8', { ignoreBOM: true, fatal: true }) : { decode: () => { throw Error('TextDecoder not available') } } );

if (typeof TextDecoder !== 'undefined') { cachedTextDecoder.decode(); };

let cachedUint8ArrayMemory0 = null;

function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

let WASM_VECTOR_LEN = 0;

const cachedTextEncoder = (typeof TextEncoder !== 'undefined' ? new TextEncoder('utf-8') : { encode: () => { throw Error('TextEncoder not available') } } );

const encodeString = (typeof cachedTextEncoder.encodeInto === 'function'
    ? function (arg, view) {
    return cachedTextEncoder.encodeInto(arg, view);
}
    : function (arg, view) {
    const buf = cachedTextEncoder.encode(arg);
    view.set(buf);
    return {
        read: arg.length,
        written: buf.length
    };
});

function passStringToWasm0(arg, malloc, realloc) {

    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }

    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = encodeString(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

let cachedDataViewMemory0 = null;

function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;

function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_export_2.set(idx, obj);
    return idx;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

const EngineFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_engine_free(ptr >>> 0, 1));

export class Engine {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        EngineFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_engine_free(ptr, 0);
    }
    /**
     * @param {string} scene_json
     * @param {string} assets_json
     * @param {string} prefs_json
     * @param {bigint} seed
     * @param {number} width
     * @param {number} height
     */
    constructor(scene_json, assets_json, prefs_json, seed, width, height) {
        const ptr0 = passStringToWasm0(scene_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(assets_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(prefs_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len2 = WASM_VECTOR_LEN;
        const ret = wasm.engine_new(ptr0, len0, ptr1, len1, ptr2, len2, seed, width, height);
        this.__wbg_ptr = ret >>> 0;
        EngineFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Screen size in CSS pixels (Unity's Screen.width / height).
     * @param {number} width
     * @param {number} height
     */
    set_screen(width, height) {
        wasm.engine_set_screen(this.__wbg_ptr, width, height);
    }
    /**
     * Pointer in screen pixels, origin bottom-left. kind: 0 down, 1 move, 2 up.
     * @param {number} id
     * @param {number} x
     * @param {number} y
     * @param {number} kind
     */
    pointer(id, x, y, kind) {
        wasm.engine_pointer(this.__wbg_ptr, id, x, y, kind);
    }
    /**
     * Keyboard / gamepad twin sticks for player 1 or 2 (values in -1..1).
     * @param {number} player
     * @param {number} mx
     * @param {number} my
     * @param {number} ax
     * @param {number} ay
     * @param {boolean} active
     */
    stick(player, mx, my, ax, ay, active) {
        wasm.engine_stick(this.__wbg_ptr, player, mx, my, ax, ay, active);
    }
    /**
     * Desktop mode: players use `direct` controls; on-screen joysticks hide.
     * Turn the running match into target practice. False until a match is up.
     * @returns {boolean}
     */
    start_practice() {
        const ret = wasm.engine_start_practice(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * [on, hits, streak] for the practice score
     * @returns {Float32Array}
     */
    practice_stats() {
        const ret = wasm.engine_practice_stats(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * AI temperament for a slot: 0 wanders, 1-3 fights (easy, normal, hard).
     * @param {number} slot
     * @param {number} level
     */
    set_ai_level(slot, level) {
        wasm.engine_set_ai_level(this.__wbg_ptr, slot, level);
    }
    /**
     * Hand an input slot (0 = P1, 1 = P2) to the wandering AI, or back.
     * @param {number} slot
     * @param {boolean} on
     */
    set_ai(slot, on) {
        wasm.engine_set_ai(this.__wbg_ptr, slot, on);
    }
    /**
     * Snake movement: you never stop; WASD / the stick set your heading.
     * @param {boolean} on
     */
    set_snake(on) {
        wasm.engine_set_snake(this.__wbg_ptr, on);
    }
    /**
     * @param {boolean} on
     */
    set_desktop(on) {
        wasm.engine_set_desktop(this.__wbg_ptr, on);
    }
    toggle_pause() {
        wasm.engine_toggle_pause(this.__wbg_ptr);
    }
    /**
     * Mouse cursor in screen pixels (origin bottom-left), or negative to clear.
     * @param {number} x
     * @param {number} y
     */
    cursor(x, y) {
        wasm.engine_cursor(this.__wbg_ptr, x, y);
    }
    /**
     * Desktop controls for player 0/1: move (-1..1), aim stick, whether the
     * throw is being charged, and whether to aim at the mouse cursor.
     * @param {number} player
     * @param {number} mx
     * @param {number} my
     * @param {number} ax
     * @param {number} ay
     * @param {boolean} aiming
     * @param {boolean} use_cursor
     * @param {boolean} active
     */
    direct(player, mx, my, ax, ay, aiming, use_cursor, active) {
        wasm.engine_direct(this.__wbg_ptr, player, mx, my, ax, ay, aiming, use_cursor, active);
    }
    /**
     * Dash button state for player 0/1 (Shift / controller A).
     * @param {number} player
     * @param {boolean} held
     */
    dash(player, held) {
        wasm.engine_dash(this.__wbg_ptr, player, held);
    }
    /**
     * @param {boolean} down
     */
    any_key(down) {
        wasm.engine_any_key(this.__wbg_ptr, down);
    }
    frame() {
        wasm.engine_frame(this.__wbg_ptr);
    }
    /**
     * Simulate a network link to the bot: one-way latency and jitter in
     * ms, packet loss 0..1, and your input delay in ticks. The rollback
     * session starts with the next match (now, if one is running).
     * @param {number} one_way_ms
     * @param {number} jitter_ms
     * @param {number} loss
     * @param {number} delay
     */
    set_lag(one_way_ms, jitter_ms, loss, delay) {
        wasm.engine_set_lag(this.__wbg_ptr, one_way_ms, jitter_ms, loss, delay);
    }
    /**
     * Read the wire between ticks (every rendered frame), so inputs that
     * arrived since the last tick aren't left waiting up to 20 ms.
     */
    net_poll() {
        wasm.engine_net_poll(this.__wbg_ptr);
    }
    lag_off() {
        wasm.engine_lag_off(this.__wbg_ptr);
    }
    /**
     * [state (0 off, 1 connecting, 2 running), one-way ms, jitter ms,
     * loss, then `LagSim::take_stats`]
     * @returns {Float32Array}
     */
    lag_stats() {
        const ret = wasm.engine_lag_stats(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Start an online match that both machines build identically: a fresh
     * game from the shared seed, through the same menu taps on the same
     * setup screen, then each switches to its own screen (which only moves
     * the camera framing and UI). You are player `me` (0 = P1, the host).
     * @param {bigint} seed
     * @param {number} map_steps
     * @param {number} me
     * @param {number} delay
     */
    net_begin(seed, map_steps, me, delay) {
        wasm.engine_net_begin(this.__wbg_ptr, seed, map_steps, me, delay);
    }
    /**
     * Leave the online match (the game stays where it is).
     */
    net_end() {
        wasm.engine_net_end(this.__wbg_ptr);
    }
    /**
     * A packet from the other player (the page's data channel).
     * @param {Uint8Array} bytes
     */
    net_recv(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.engine_net_recv(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * The next packet to send, or empty when there are none.
     * @returns {Uint8Array}
     */
    net_pop() {
        const ret = wasm.engine_net_pop(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Link events since the last call, comma separated ("synced",
     * "interrupted", "resumed", "disconnected", "desync", "ended").
     * @returns {string}
     */
    net_events() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_net_events(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * [online 0/1, running 0/1, then `LagSim::take_stats`]
     * @returns {Float32Array}
     */
    net_stats() {
        const ret = wasm.engine_net_stats(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Build draw records; returns their f32 count (read via records_ptr).
     * How far (0..1 of a tick) the host is past the last fixed step, so
     * motion graphics draw between ticks.
     * @param {number} a
     */
    set_subtick(a) {
        wasm.engine_set_subtick(this.__wbg_ptr, a);
    }
    /**
     * Debug: shatter a box's worth of shards at a world point, flying
     * back along (vx, vy), as if a throw at that speed hit something.
     * @param {number} x
     * @param {number} y
     * @param {number} vx
     * @param {number} vy
     */
    debug_shatter(x, y, vx, vy) {
        wasm.engine_debug_shatter(this.__wbg_ptr, x, y, vx, vy);
    }
    /**
     * Profile since the last call: [ticks, frames, (mean ms, worst ms) per
     * phase in `phase_names()` order]. Resets.
     * @returns {Float32Array}
     */
    profile() {
        const ret = wasm.engine_profile(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {string}
     */
    phase_names() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_phase_names(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    render() {
        const ret = wasm.engine_render(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    records_ptr() {
        const ret = wasm.engine_records_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Text strings for text records: {node: [text, style]}
     * A checksum of the visible texts: the host refetches `texts()` only
     * when this changes (it would otherwise parse them 50 times a second).
     * @returns {number}
     */
    texts_version() {
        const ret = wasm.engine_texts_version(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {string}
     */
    texts() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_texts(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * {textures: [file], meshes: [key], materials: [[key, [r,g,b,a]]], font}
     * @returns {string}
     */
    tables() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_tables(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One-shot sounds since the last call: [[clip, volume]]
     * @returns {string}
     */
    drain_sounds() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_drain_sounds(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Synthesised cues since the last call: [[name, pan, amount]]
     * @returns {string}
     */
    drain_cues() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_drain_cues(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Continuous mix inputs, every frame: [charge p0, pan p0, charge p1,
     * pan p1, lull 0..1, paused 0/1, in match 0/1]
     * @returns {Float32Array}
     */
    mix() {
        const ret = wasm.engine_mix(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Music sources: [[clip, playing, volume]] (muted when Music is OFF)
     * @returns {string}
     */
    music() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_music(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    take_prefs() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_take_prefs(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {string}
     */
    state() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.engine_state(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Screen-space quad of the first active node with this name (for tests/automation).
     * @param {string} name
     * @returns {Float32Array}
     */
    locate(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.engine_locate(this.__wbg_ptr, ptr0, len0);
        var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);

            } catch (e) {
                if (module.headers.get('Content-Type') != 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else {
                    throw e;
                }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);

    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };

        } else {
            return instance;
        }
    }
}

function __wbg_get_imports() {
    const imports = {};
    imports.wbg = {};
    imports.wbg.__wbg_static_accessor_PERFORMANCE_359f854c55986b6f = function() {
        const ret = performance;
        return ret;
    };
    imports.wbg.__wbg_now_ba16c792ede337fa = function(arg0) {
        const ret = arg0.now();
        return ret;
    };
    imports.wbg.__wbg_crypto_8f90fdde9566e2ad = function(arg0) {
        const ret = arg0.crypto;
        return ret;
    };
    imports.wbg.__wbindgen_is_object = function(arg0) {
        const val = arg0;
        const ret = typeof(val) === 'object' && val !== null;
        return ret;
    };
    imports.wbg.__wbg_process_8eb2777bc1b6b0fa = function(arg0) {
        const ret = arg0.process;
        return ret;
    };
    imports.wbg.__wbg_versions_50a627196d924b94 = function(arg0) {
        const ret = arg0.versions;
        return ret;
    };
    imports.wbg.__wbg_node_fe444c3146177840 = function(arg0) {
        const ret = arg0.node;
        return ret;
    };
    imports.wbg.__wbindgen_is_string = function(arg0) {
        const ret = typeof(arg0) === 'string';
        return ret;
    };
    imports.wbg.__wbg_require_118cb4d5bf61ca1d = function() { return handleError(function () {
        const ret = module.require;
        return ret;
    }, arguments) };
    imports.wbg.__wbindgen_is_function = function(arg0) {
        const ret = typeof(arg0) === 'function';
        return ret;
    };
    imports.wbg.__wbindgen_string_new = function(arg0, arg1) {
        const ret = getStringFromWasm0(arg0, arg1);
        return ret;
    };
    imports.wbg.__wbg_msCrypto_31602000d4eb6ef9 = function(arg0) {
        const ret = arg0.msCrypto;
        return ret;
    };
    imports.wbg.__wbg_randomFillSync_bf004fc1e39ad54a = function() { return handleError(function (arg0, arg1) {
        arg0.randomFillSync(arg1);
    }, arguments) };
    imports.wbg.__wbg_getRandomValues_47a210009a601d67 = function() { return handleError(function (arg0, arg1) {
        arg0.getRandomValues(arg1);
    }, arguments) };
    imports.wbg.__wbg_now_d3cbc9581625f686 = function(arg0) {
        const ret = arg0.now();
        return ret;
    };
    imports.wbg.__wbg_self_bf91bf94d9e04084 = function() { return handleError(function () {
        const ret = self.self;
        return ret;
    }, arguments) };
    imports.wbg.__wbg_window_52dd9f07d03fd5f8 = function() { return handleError(function () {
        const ret = window.window;
        return ret;
    }, arguments) };
    imports.wbg.__wbg_globalThis_05c129bf37fcf1be = function() { return handleError(function () {
        const ret = globalThis.globalThis;
        return ret;
    }, arguments) };
    imports.wbg.__wbg_global_3eca19bb09e9c484 = function() { return handleError(function () {
        const ret = global.global;
        return ret;
    }, arguments) };
    imports.wbg.__wbindgen_is_undefined = function(arg0) {
        const ret = arg0 === undefined;
        return ret;
    };
    imports.wbg.__wbg_newnoargs_1ede4bf2ebbaaf43 = function(arg0, arg1) {
        const ret = new Function(getStringFromWasm0(arg0, arg1));
        return ret;
    };
    imports.wbg.__wbg_call_a9ef466721e824f2 = function() { return handleError(function (arg0, arg1) {
        const ret = arg0.call(arg1);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_get_ef828680c64da212 = function() { return handleError(function (arg0, arg1) {
        const ret = Reflect.get(arg0, arg1);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_call_3bfa248576352471 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = arg0.call(arg1, arg2);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_getTime_41225036a0393d63 = function(arg0) {
        const ret = arg0.getTime();
        return ret;
    };
    imports.wbg.__wbg_new0_218ada33b570be35 = function() {
        const ret = new Date();
        return ret;
    };
    imports.wbg.__wbg_buffer_ccaed51a635d8a2d = function(arg0) {
        const ret = arg0.buffer;
        return ret;
    };
    imports.wbg.__wbg_newwithbyteoffsetandlength_7e3eb787208af730 = function(arg0, arg1, arg2) {
        const ret = new Uint8Array(arg0, arg1 >>> 0, arg2 >>> 0);
        return ret;
    };
    imports.wbg.__wbg_new_fec2611eb9180f95 = function(arg0) {
        const ret = new Uint8Array(arg0);
        return ret;
    };
    imports.wbg.__wbg_set_ec2fcf81bc573fd9 = function(arg0, arg1, arg2) {
        arg0.set(arg1, arg2 >>> 0);
    };
    imports.wbg.__wbg_newwithlength_76462a666eca145f = function(arg0) {
        const ret = new Uint8Array(arg0 >>> 0);
        return ret;
    };
    imports.wbg.__wbg_subarray_975a06f9dbd16995 = function(arg0, arg1, arg2) {
        const ret = arg0.subarray(arg1 >>> 0, arg2 >>> 0);
        return ret;
    };
    imports.wbg.__wbindgen_debug_string = function(arg0, arg1) {
        const ret = debugString(arg1);
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
        getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
    };
    imports.wbg.__wbindgen_throw = function(arg0, arg1) {
        throw new Error(getStringFromWasm0(arg0, arg1));
    };
    imports.wbg.__wbindgen_memory = function() {
        const ret = wasm.memory;
        return ret;
    };
    imports.wbg.__wbindgen_init_externref_table = function() {
        const table = wasm.__wbindgen_export_2;
        const offset = table.grow(4);
        table.set(0, undefined);
        table.set(offset + 0, undefined);
        table.set(offset + 1, null);
        table.set(offset + 2, true);
        table.set(offset + 3, false);
        ;
    };

    return imports;
}

function __wbg_init_memory(imports, memory) {

}

function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    __wbg_init.__wbindgen_wasm_module = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;


    wasm.__wbindgen_start();
    return wasm;
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (typeof module !== 'undefined') {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();

    __wbg_init_memory(imports);

    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }

    const instance = new WebAssembly.Instance(module, imports);

    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (typeof module_or_path !== 'undefined') {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (typeof module_or_path === 'undefined') {
        module_or_path = new URL('neobrawl_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    __wbg_init_memory(imports);

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync };
export default __wbg_init;
