// AudioWorkletProcessor hosting the Ferrodrome DSP (ferrodrome-worklet.wasm).
//
// The main thread compiles the wasm module and hands it over through
// processorOptions; the module has no imports, so instantiation is synchronous.
//
// Messages on `port` are small arrays:
//   [0, track, paramId, value]   set a track parameter
//   [1, paramId, value]          set a global parameter
//   [2, track, step, on]         draw or erase a step
//   [3]                          snap smoothed parameters
//   [4, track, paramId, locked]  lock a parameter against RND, MUTATE and VARIANCE
//   [5, on]                      start/stop the sequencer
//   [6, track, on]               hold a machine running (RUN)
//
// Whenever the engine status changes (step, drive meters, MUTATE, VARIANCE)
// the processor posts it back as a Float32Array.

const STATUS_LEN = 602;

class FerrodromeProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.dsp = new WebAssembly.Instance(options.processorOptions.module, {}).exports;
    this.handle = this.dsp.fd_new(sampleRate);
    this.views = null;
    this.lastStatus = new Float32Array(STATUS_LEN);
    this.port.onmessage = (e) => this.onMessage(e.data);
  }

  onMessage(m) {
    const d = this.dsp;
    const h = this.handle;
    switch (m[0]) {
      case 0: d.fd_set_param(h, m[1], m[2], m[3]); break;
      case 1: d.fd_set_global(h, m[1], m[2]); break;
      case 2: d.fd_set_step(h, m[1], m[2], m[3] ? 1 : 0); break;
      case 3: d.fd_snap(h); break;
      case 4: d.fd_set_lock(h, m[1], m[2], m[3] ? 1 : 0); break;
      case 5: d.fd_play(h, m[1] ? 1 : 0); break;
      case 6: d.fd_hold(h, m[1], m[2] ? 1 : 0); break;
    }
  }

  // (Re)create the Float32Array views if wasm memory was replaced by a grow.
  outputViews(frames) {
    const buffer = this.dsp.memory.buffer;
    const v = this.views;
    if (v === null || v.buffer !== buffer || v.frames !== frames) {
      this.views = {
        buffer,
        frames,
        left: new Float32Array(buffer, this.dsp.fd_left(this.handle), frames),
        right: new Float32Array(buffer, this.dsp.fd_right(this.handle), frames),
        status: new Float32Array(buffer, this.dsp.fd_status(this.handle), STATUS_LEN),
      };
    }
    return this.views;
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    const frames = out[0].length;
    this.dsp.fd_render(this.handle, frames);
    const v = this.outputViews(frames);
    out[0].set(v.left);
    if (out.length > 1) out[1].set(v.right);
    let changed = false;
    for (let i = 0; i < STATUS_LEN; i++) {
      if (v.status[i] !== this.lastStatus[i]) { changed = true; break; }
    }
    if (changed) {
      this.lastStatus.set(v.status);
      this.port.postMessage(this.lastStatus.slice());
    }
    return true;
  }
}

registerProcessor("ferrodrome", FerrodromeProcessor);
