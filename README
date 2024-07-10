# rip8

## Description

rip8 is an interpreter for the CHIP-8 programming language. It is intentded to observe the semantics of the language as presented in the COSMAC VIP instruction manual (plus undocumented instructions present in the original intrepreter), but deviates in some implementation details to adcommodate modern ROMs, such as:

- Support for CHIP-8 as well as S-CHIP instruction semantics (only affects instructions `8XY6`, `8XYE`, `FX55` and `FX65`).
- Deeper call stack.
- Out-of-memory stack so that programs can make use of up to 3584 bytes of memory (4096 - 256 reserved for font data).
- Customizable clock frequency.
- Customizable load/start address.

## Build

### Manually with Cargo

Make sure you have `libsdl2-dev` installed on your system, if on Ubuntu, you can install it with:

```
sudo apt install libsdl2-dev
```

then run:

```
cargo build
cargo test
```

### Snap

Simply move to the root of the repo and run:

```
snapcraft
sudo snap install rip8_* --dangerous # We need this flag since the snap will not be signed
```

## Running

Keep in mind that when running a ROM, the upper-left portion of your keyboard (keys `1234QWERASDFZXCV` if using a QWERTY keyboard layout) will be used for input, using COSMAC VIP's keyboard layout.

### Manually with Cargo

The runtime library `libsdl2` is required in order to run the interpreter, you can install it with:

```
sudo apt install libsdl2-2.0-0
```

then run:

```
cargo run -- <path to your ROM>
```

### Snap

Simply run:

```
rip8 <path to your ROM>
```

### Command Line Options

You can list available runtime options with:

```
cargo run -- -h
```

#### Controlling frequency

The interpreter will execute 540 instructions/second. You can customize this value to your needs with the `-f` option, but keep in mind that since timer registers are decremented at 60Hz, you will get more accurate results when setting frequency to multiples of 60.

