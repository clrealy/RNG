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
seq 1 25 > nums.txt
printf 'one two  three\n\tfour\nfive' > words.txt
printf 'a\0b\0c\0' > zero.txt

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

# yes (piped into GNU head so it stops)
checkpipe() { # $1 = shell pipeline using the tool name "T"
  env PATH="$TMP/gnu:/usr/bin:/bin" sh -c "$1" > gnu.out 2> gnu.err; g=$?
  env PATH="$TMP/bin:/usr/bin:/bin" sh -c "$1" > rng.out 2> rng.err; r=$?
  if cmp -s gnu.out rng.out && [ "$g" = "$r" ]; then pass=$((pass+1));
  else fail=$((fail+1)); echo "FAIL (pipe): $1"; diff gnu.out rng.out | head -5; fi
}
checkpipe 'yes | /usr/bin/head -n 3'
checkpipe 'yes roll the dice | /usr/bin/head -n 2'
checkpipe 'yes -- -n | /usr/bin/head -n 2'
checkpipe 'printf "x\ny\nz\n" | wc'
checkpipe 'printf "x\ny\nz\n" | wc -l'
checkpipe 'printf "1\n2\n3\n" | tail -n 1'
checkpipe 'printf "1\n2\n3\n" | head -c 3'
checkpipe 'wc < a.txt'
checkpipe 'cd adir && pwd'
checkpipe 'cd adir && pwd -L'

# basename / dirname
check basename /usr/lib/libc.so.6
check basename /usr/lib/
check basename ///
check basename file.txt .txt
check basename .txt .txt
check basename -a /x/y /a/b/
check basename -s .rs src/main.rs src/util.rs
check basename -z /x/y
check basename
check basename a b c
check dirname /usr/lib/libc.so.6
check dirname /usr/lib/
check dirname file
check dirname /file
check dirname ///
check dirname a//b// x/y
check dirname
check dirname -z a/b

# head
check head nums.txt
check head -n 3 nums.txt
check head -3 nums.txt
check head -n -20 nums.txt
check head -c 10 nums.txt
check head -c -40 nums.txt
check head -n 2 a.txt nums.txt
check head -q -n 2 a.txt nums.txt
check head -v -n 1 nums.txt
check head -n 2 nope.txt nums.txt
check head -n x nums.txt
check head -z -n 2 zero.txt
check head -n 1K nums.txt
check head adir

# tail
check tail nums.txt
check tail -n 3 nums.txt
check tail -3 nums.txt
check tail -n +20 nums.txt
check tail -c 10 nums.txt
check tail -c +60 nums.txt
check tail -n 1 a.txt
check tail -n 2 a.txt nums.txt
check tail -q -n 1 a.txt nums.txt
check tail -n 2 nope.txt nums.txt
check tail -z -n 1 zero.txt
check tail -n 0 nums.txt

# wc
check wc a.txt
check wc nums.txt words.txt
check wc -l nums.txt
check wc -w words.txt
check wc -c words.txt
check wc -m words.txt
check wc -L words.txt
check wc -lwcmL a.txt b.txt
check wc empty.txt
check wc nope.txt nums.txt
check wc adir
check wc -l nums.txt words.txt

echo "passed: $pass  failed: $fail"
[ "$fail" = 0 ]
