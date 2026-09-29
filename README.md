# slop-PMD

An emulator of the Tesla PMD 85 family of 8-bit Czechoslovak home computers
(PMD 85-1, 85-2, 85-2A, 85-3), written in Rust.

The CPU core is a cycle-counted Intel 8080 validated against the classic
CP/M diagnostic ROMs (`8080PRE`, `TST8080`, `CPUTEST`, `8080EXM`).
The machine side follows MAME's `pmd85.cpp` / `pmd85_m.cpp` driver
(memory maps including the boot-time startup shadow map, keyboard matrix
through the system 8255, speaker and keyboard LED, PMD 85-3 ROM/VRAM banking).

## Crates

| Crate | Contents |
|---|---|
| `crates/pmd85-core` | Pure emulation logic: i8080 CPU, memory maps, 8255/8253/8251 chips, keyboard matrix, VRAM decode, machine glue. No dependencies. |
| `crates/pmd85-app` | Desktop application: winit window + wgpu blit of the 288x256 framebuffer, 50 Hz real-time pacing, host keyboard mapping. |

## Running

```
cargo run -p pmd85-app --release
```

Options:

- `--model <name>` — `85-1`, `85-2`, `85-2A` or `85-3` (default `85-3`)
- `--monitor <path>` — monitor ROM image (default `Rom/<model's monitor>.rom`)
- `--rom-module <path>` — optional 8 KiB ROM module (`.rmm`) at 0x8000
- `--mute` — disable speaker output

Speaker audio plays through the default output device. The PMD 85 sound
circuit is emulated as a whole: PC2 of the system 8255 drives the piezo
transducer directly (software-generated square wave, e.g. BASIC beeps),
while PC0 and PC1 gate fixed 1 kHz / 4 kHz tones derived from the video
divider — which is how the monitor ROMs make their key clicks. The app
converts the cycle-stamped speaker edges into a mono stream (any output
sample rate, exact integer timing) and feeds cpal through a small ring
buffer; without an audio device it runs silent.

The Monitor-3 command line works: type e.g. `DUMP 0100` (addresses are
4-digit hex) and press Enter. `JUMP FFF0` switches the PMD 85-3 into the
PMD 85-2 compatibility mode.

Keyboard mapping (physical, PMD 85 is QWERTZ — host Y/Z map to PMD Z/Y):

- letters, digits, `Space`, `Enter`, `Tab`, `Backspace`, `Delete`, `Insert`,
  arrows, `Home`/`End` map directly
- `Shift` → SHIFT, `Esc`/`Ctrl` → STOP
- `F1`..`F12` → function keys `K0`..`K11`

## ROM images

Monitor ROMs live in `Rom/` (see `Rom/rom-list.txt`). They are not part of
this repository's source; the boot tests require them to be present.

## Testing

```
cargo test --workspace
```

- `pmd85-core` unit tests: every instruction group, memory maps, chips,
  keyboard, VRAM decode, speaker edge log and edge-to-sample expansion
- CP/M diagnostic ROMs run headless through the machine (`diag_tests`);
  `8080EXM` is `#[ignore]`d (run it in release: ~20 s)
- `boot_tests`: every model boots its monitor ROM to a working command
  line, keyboard echo lands in VRAM, Monitor-3 executes a real `DUMP`
  command end-to-end, and keypresses produce the speaker click
- `module_tests`: the monitor BIOS itself detects an attached ROM module
  through the module 8255, copies it to RAM 0 and runs it (basic1/2/2A/3,
  one test per model, plus a no-module control)
- `pmd85-app`: ring-buffer and live audio-stream tests (skipped silently
  when no output device is present)

## Debug tools

`crates/pmd85-core/examples/`:

- `boot_dump` — boots a model, optionally presses keys, writes the decoded
  screen to a PNG and a coarse ASCII rendering (e.g.
  `cargo run -p pmd85-core --example boot_dump -- --model 85-3 --frames 400
  --press "DUMP 0100[ENTER]" --out screen.png`); the press string also
  supports `[SHIFT]`/`[UNSHIFT]` and `[WAIT:n]` to idle n frames (useful
  after a command that boots a ROM module)
- `kbd_trace` — IN/OUT, VRAM and RAM-state tracing while typing

## Status

Done: CPU, memory, keyboard, video, boot of all four models, monitor
command execution, ROM module support (monitor BIOS detects/transfers/runs
BASIC modules), wgpu display, speaker audio.

Not yet: cassette/serial I/O beyond stubs, debugger UI, settings, tape/disk
image support.

## References

- [pmd85.borik.net wiki](https://pmd85.borik.net/wiki/) — hardware docs
- MAME `tesla/pmd85.cpp` driver (memory maps, I/O decoding)
- superzazu/8080 `i8080.c` (undocumented flag semantics)
