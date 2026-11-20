<p align="center">
  <img src="assets/logo.png" alt="RNG logo: a die with R, N and G on its faces" width="180">
</p>

<h1 align="center">RNG's Not GNU!!!</h1>

<p align="center">The GNU userland, remade from scratch in Rust. One binary, every tool.</p>

---

## What is this

RNG is a from-scratch remake of the GNU tools: coreutils first, then the bigger stuff
(grep, sed, find, tar, make…), and eventually libc and a toolchain. It's written in
Rust, and it's **not** a fork: no GNU code goes in here, just the man pages and specs.

Everything ships as **one binary** (BusyBox-style). Run a tool as `rng <tool>`, or
symlink `rng` to the tool's name and call it directly.

## Tools so far

| Tool    | Status | Notes |
|---------|--------|-------|
| `cat`   | ✅     | all GNU flags: `-A -b -e -E -n -s -t -T -u -v` + long forms |
| `echo`  | ✅     | `-n -e -E`, every GNU escape (`\c`, `\0NNN`, `\xHH`…) |
| `true`  | ✅     | |
| `false` | ✅     | |

Next up: `yes`, `pwd`, `basename`, `dirname`, `head`, `tail`, `wc`.

## Build it

```sh
cargo build --release
./target/release/rng --list
./target/release/rng echo -e 'roll\tthe dice'
```

Use it like the real thing:

```sh
mkdir -p ~/.local/rng && cd ~/.local/rng
for t in $(/path/to/rng --list); do ln -sf /path/to/rng "$t"; done
export PATH="$HOME/.local/rng:$PATH"   # now `cat` is RNG's cat
```

## Testing

Unit tests:

```sh
cargo test
```

GNU compatibility: runs the same commands through GNU and through RNG and checks that
stdout, stderr and the exit code all match.

```sh
cargo build --release && sh tests/gnu-compat.sh
```

Every new tool or flag gets a line in `tests/gnu-compat.sh`. If GNU and RNG disagree,
RNG is wrong.

## Adding a tool

1. Make `src/tools/<name>.rs` with `pub fn main(prog: &str, args: Vec<OsString>) -> u8`.
2. Register it in `ALL` in `src/tools/mod.rs`.
3. Add compat checks to `tests/gnu-compat.sh`.

Don't read GNU's source while writing a tool. It's GPL, and this project isn't.

## License

[RNG Roller License (RRL) v1.0](License.txt). Do whatever you want, just give credit,
and don't close-source it (you're allowed, but why the hell would you?).

Roll the dice. Don't die. Ok?
