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

| Tool       | Notes |
|------------|-------|
| `basename` | `-a -s -z`, `NAME SUFFIX` form |
| `cat`      | every GNU flag: `-A -b -e -E -n -s -t -T -u -v` + long forms |
| `cp`       | `-a -d -f -i -l -L -n -p -P -r -s -t -T -u -v`, `--preserve=…`, keeps hard links together |
| `dirname`  | `-z`, any number of names |
| `echo`     | `-n -e -E`, every GNU escape (`\c`, `\0NNN`, `\xHH`…) |
| `false`    | |
| `head`     | `-n`/`-c` with `-N` ("all but the last N"), `-q -v -z`, old `-5` style, size suffixes |
| `ls`       | `-a -A -l -h -1 -C -x -m -R -d -t -S -X -r -U -F -p -i -s -n -g -o -G -u -c -L -q -N -w -T`, colors, `--time-style`, `--group-directories-first` |
| `mkdir`    | `-p -v`, `-m` with octal or symbolic modes (`u=rwx,g=rx`) |
| `mv`       | `-f -i -n -t -T -u -v`, falls back to copy + delete across filesystems |
| `pwd`      | `-L -P` |
| `rm`       | `-f -i -I -r -d -v`. **Will never remove `/`** (see below) |
| `tail`     | `-n`/`-c` with `+N` ("start at N"), `-f`, `-q -v -z`, old `-5` style |
| `touch`    | `-a -m -c -h -r`, `-t STAMP`, `-d` (`@SECONDS`, `YYYY-MM-DD HH:MM:SS`, `now`) |
| `true`     | |
| `wc`       | `-l -w -c -m -L`, same column widths as GNU |
| `yes`      | as fast as GNU's |

Every tool is checked against GNU in both the C and UTF-8 locales (228/228 matching):
output, error messages, exit codes, and for `cp`/`mv`/`rm`/`mkdir`/`touch` the resulting
file tree too.

Next up: `chmod`, `chown`, `ln`, `cut`, `sort`, `uniq`, `tr`.

## rm and `/`

```console
$ rm -rf /
Bye
```

rng's `rm` refuses to touch `/` no matter how you ask: `rm -rf /`, `--no-preserve-root`,
`//`, `/.`, `/usr/..`, a symlink that points at `/`, or `rm -rf /*` after the shell
expands it. It prints `Bye`, exits 1, and deletes **nothing**, not even the other
arguments. There's no flag to turn this off. (Tested for real inside a throwaway chroot.)

## Where rng is different on purpose

- `rm` never removes `/`, see above.
- `cp -r dir dir/sub` fails before copying anything. GNU copies part of it first, then fails.

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
LC_ALL=C.UTF-8 sh tests/gnu-compat.sh   # GNU changes some quotes in UTF-8
```

Every new tool or flag gets a line in `tests/gnu-compat.sh`. If GNU and RNG disagree,
RNG is wrong.

## Adding a tool

1. Make `src/tools/<name>.rs` with `pub fn main(prog: &str, args: Vec<OsString>) -> u8`.
2. Register it in `ALL` in `src/tools/mod.rs`.
3. Add compat checks to `tests/gnu-compat.sh`.

Don't read GNU's source while writing a tool. It's GPL, and this project isn't.

## License

[RNG Roller License (RRL) v1.0](LICENSE). Do whatever you want, just give credit,
and don't close-source it (you're allowed, but why the hell would you?).

Roll the dice. Don't die. Ok?
