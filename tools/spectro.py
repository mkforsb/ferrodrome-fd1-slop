#!/usr/bin/env python3
"""Side-by-side spectrograms and average spectra of renders and reference
recordings, for comparing a machine model with the real thing.

    tools/spectro.py out.png render.wav[:label] reference.wav[:label] ...

Each file gets a spectrogram (0–8 kHz, log-ish frequency) and all of them
share one plot of their long-term average spectrum, normalized to 0 dB peak.
"""
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from scipy.io import wavfile
from scipy.signal import spectrogram, welch


def load(path, seconds=10.0):
    fs, x = wavfile.read(path)
    x = x.astype(np.float64)
    if x.ndim > 1:
        x = x.mean(axis=1)
    x /= max(np.abs(x).max(), 1e-9)
    return fs, x[: int(seconds * fs)]


def main():
    out = sys.argv[1]
    items = []
    for arg in sys.argv[2:]:
        path, _, label = arg.partition(":")
        items.append((path, label or path.rsplit("/", 1)[-1]))
    n = len(items)
    fig, axes = plt.subplots(n + 1, 1, figsize=(11, 2.6 * (n + 1)))
    for ax, (path, label) in zip(axes, items):
        fs, x = load(path)
        f, t, s = spectrogram(x, fs, nperseg=2048, noverlap=1536)
        keep = f <= 8000
        ax.pcolormesh(t, f[keep], 10 * np.log10(s[keep] + 1e-12), shading="auto", cmap="magma",
                      vmin=10 * np.log10(s.max() + 1e-12) - 80)
        ax.set_yscale("symlog", linthresh=500)
        ax.set_ylim(20, 8000)
        ax.set_title(label, fontsize=9)
        ax.set_ylabel("Hz")
    ax = axes[-1]
    for path, label in items:
        fs, x = load(path)
        f, p = welch(x, fs, nperseg=8192)
        db = 10 * np.log10(p + 1e-18)
        ax.semilogx(f[1:], db[1:] - db.max(), label=label, lw=1)
    ax.set_xlim(20, 16000)
    ax.set_ylim(-90, 3)
    ax.grid(True, which="both", alpha=0.3)
    ax.legend(fontsize=8)
    ax.set_title("long-term average spectrum", fontsize=9)
    fig.tight_layout()
    fig.savefig(out, dpi=72)


if __name__ == "__main__":
    main()
