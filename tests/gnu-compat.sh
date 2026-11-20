#!/bin/sh
# Runs the same commands through GNU and through rng and checks that
# stdout, stderr and the exit code all match.
#
#   cargo build --release && sh tests/gnu-compat.sh
#
# Needs GNU coreutils installed (any normal Linux distro has them).

RNG="${RNG:-$(pwd)/target/release/rng}"
TMP="$(pwd)/tests/.gnu-compat-tmp"
rm -rf "$TMP" && mkdir -p "$TMP/bin" && cd "$TMP" || exit 1

# Put each tool in its own folder (rng symlinks in bin/, real GNU ones in gnu/)
# and run them through PATH, so both see argv[0] = "cat" and errors look the same.
mkdir -p gnu
for t in $("$RNG" --list); do
  ln -sf "$RNG" "bin/$t"
  for d in /usr/bin /bin /usr/local/bin; do
    [ -x "$d/$t" ] && { ln -sf "$d/$t" "gnu/$t"; break; }
  done
done

printf 'hello\nworld\n\n\n\nlast line no newline' > a.txt
printf 'tab\there\n\001ctrl\177del\n\303\251 utf8\n' > b.txt
: > empty.txt
mkdir -p adir

pass=0; fail=0
check() {
  tool="$1"; shift
  env PATH="$TMP/gnu" "$tool" "$@" > gnu.out 2> gnu.err; g=$?
  env PATH="$TMP/bin" "$tool" "$@" > rng.out 2> rng.err; r=$?
  if cmp -s gnu.out rng.out && cmp -s gnu.err rng.err && [ "$g" = "$r" ]; then
    pass=$((pass+1))
  else
    fail=$((fail+1)); echo "FAIL: $tool $*"
    cmp -s gnu.out rng.out || { echo "  stdout differs"; diff gnu.out rng.out | head -5; }
    cmp -s gnu.err rng.err || { echo "  stderr differs"; diff gnu.err rng.err | head -5; }
    [ "$g" = "$r" ] || echo "  exit: gnu=$g rng=$r"
  fi
}

# echo
check echo
check echo hello world
check echo -n no newline
check echo -e 'tab\there\nnewline'
check echo -e 'oct \0101 hex \x41\x4a bad \xZ'
check echo -e 'stop\cnot printed'
check echo -E 'raw\tstays'
check echo -nx literal
check echo -- dashes
check echo -neE 'last wins\t'
check echo -e 'trailing\'

# true / false
check true
check false
check true --whatever
check false nope

# cat
check cat a.txt
check cat a.txt b.txt
check cat -n a.txt b.txt
check cat -b a.txt
check cat -s a.txt
check cat -E a.txt
check cat -T b.txt
check cat -v b.txt
check cat -A b.txt
check cat -et a.txt b.txt
check cat -nsE a.txt
check cat --number --show-ends a.txt
check cat empty.txt
check cat nope.txt
check cat a.txt nope.txt b.txt
check cat adir
check cat -z a.txt
check cat -- a.txt

echo "passed: $pass  failed: $fail"
[ "$fail" = 0 ]
