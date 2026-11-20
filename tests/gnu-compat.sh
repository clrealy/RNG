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
  env PATH="$TMP/gnu" "$tool" "$@" > "$TMP/gnu.out" 2> "$TMP/gnu.err"; g=$?
  env PATH="$TMP/bin" "$tool" "$@" > "$TMP/rng.out" 2> "$TMP/rng.err"; r=$?
  ( cd "$TMP"
  if cmp -s gnu.out rng.out && cmp -s gnu.err rng.err && [ "$g" = "$r" ]; then
    :
  else
    echo "FAIL: $tool $*"
    cmp -s gnu.out rng.out || { echo "  stdout differs"; diff gnu.out rng.out | head -5; }
    cmp -s gnu.err rng.err || { echo "  stderr differs"; diff gnu.err rng.err | head -5; }
    [ "$g" = "$r" ] || echo "  exit: gnu=$g rng=$r"
  fi ) > "$TMP/check.log"
  if grep -q FAIL "$TMP/check.log"; then fail=$((fail+1)); cat "$TMP/check.log"; else pass=$((pass+1)); fi
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

# ---------- tools that change the filesystem ----------
# Each case runs in two identical fresh sandboxes (one per implementation);
# we compare output, exit code AND the resulting file tree.
fixture() {
  /usr/bin/mkdir -p src/sub src/empty dst
  printf 'hi\n' > src/a.txt; printf 'yo\n' > src/sub/b.txt; printf 'x' > f1; printf 'yy' > f2
  /usr/bin/ln -s a.txt src/link; /usr/bin/ln src/a.txt src/hard
  /usr/bin/chmod 640 src/a.txt; /usr/bin/chmod 755 src/sub
  /usr/bin/touch -d '2020-01-02 03:04:05' src/a.txt src/sub/b.txt f1
}
snapshot() {
  /usr/bin/find . -path ./out -prune -o -path ./err -prune -o -printf '%p %y %m %s %l %n\n' | LC_ALL=C /usr/bin/sort
  /usr/bin/find . -type f ! -name out ! -name err -exec /usr/bin/md5sum {} + | LC_ALL=C /usr/bin/sort
}
checkfs() {
  for impl in gnu bin; do
    /usr/bin/rm -rf "$TMP/fs-$impl"; /usr/bin/mkdir "$TMP/fs-$impl"
    ( cd "$TMP/fs-$impl" && fixture && env PATH="$TMP/$impl" "$@" > out 2> err; echo $? > code )
    ( cd "$TMP/fs-$impl" && snapshot > "$TMP/snap-$impl" )
  done
  if cmp -s fs-gnu/out fs-bin/out && cmp -s fs-gnu/err fs-bin/err && cmp -s fs-gnu/code fs-bin/code && cmp -s snap-gnu snap-bin; then
    pass=$((pass+1))
  else
    fail=$((fail+1)); echo "FAIL (fs): $*"
    diff fs-gnu/out fs-bin/out | head -5; diff fs-gnu/err fs-bin/err | head -5
    diff fs-gnu/code fs-bin/code; diff snap-gnu snap-bin | head -8
  fi
}

# mkdir
checkfs mkdir newdir
checkfs mkdir -p a/b/c
checkfs mkdir -pv x/y
checkfs mkdir -m 700 secret
checkfs mkdir -m u=rwx,g=rx,o= m1
checkfs mkdir -m a+w m2
checkfs mkdir -m go-rwx m3
checkfs mkdir src
checkfs mkdir -p src/sub
checkfs mkdir nope/deeper
checkfs mkdir -m bogus m4
checkfs mkdir

# touch (times differ run to run, so only fixed ones are compared)
checkfs touch newfile
checkfs touch -c ghost
checkfs touch nope/ghost
checkfs touch
checkfs touch -d '2001-02-03 04:05:06' f2 brandnew
checkfs touch -t 200102030405.06 f2
checkfs touch -d @0 f2
checkfs touch -r src/a.txt f2
checkfs touch -d 'not a date' f2

# rm
checkfs rm f1
checkfs rm f1 f2 nope
checkfs rm -f nope
checkfs rm src
checkfs rm -r src
checkfs rm -rv src
checkfs rm -d src/empty
checkfs rm -d src/sub
checkfs rm -rf src dst
checkfs rm -v f1 src/link
checkfs rm .
checkfs rm -rf src/..
checkfs rm
checkfs rm -f

# cp
checkfs cp f1 f3
checkfs cp f1 f2 dst
checkfs cp -v f1 dst/
checkfs cp src dst
checkfs cp -r src dst
checkfs cp -rv src copy
checkfs cp -a src copy
checkfs cp -n f1 f2
checkfs cp -p src/a.txt keep
checkfs cp f1
checkfs cp
checkfs cp f1 f2 f3
checkfs cp nope f3
checkfs cp f1 f1
checkfs cp -t dst f1 f2
checkfs cp -T f1 f3
checkfs cp -l f1 hardcopy
checkfs cp -s f1 softcopy
checkfs cp src/link linkcopy
checkfs cp -P src/link linkcopy
checkfs cp f1 src

# mv
checkfs mv f1 f3
checkfs mv f1 f2 dst
checkfs mv -v f1 dst
checkfs mv src moved
checkfs mv -n f1 f2
checkfs mv f1 f1
checkfs mv src src/sub
checkfs mv nope x
checkfs mv f1
checkfs mv
checkfs mv f1 src/empty
checkfs mv src/empty f1
checkfs mv -t dst f1 f2
checkfs mv -T src/empty dst

# ls (fixed times and sizes so every column is predictable)
LSD="$TMP/lsdir"; /usr/bin/rm -rf "$LSD"; /usr/bin/mkdir -p "$LSD/sub/deep" "$LSD/.hidden"
printf 'aaaa' > "$LSD/b.txt"; printf 'a' > "$LSD/A.md"; printf 'aaaaaaaaaa' > "$LSD/c"
printf 'z' > "$LSD/sub/z"; printf '#!/bin/sh\n' > "$LSD/run.sh"; /usr/bin/chmod 755 "$LSD/run.sh"
/usr/bin/ln -s b.txt "$LSD/link"; /usr/bin/ln -s nowhere "$LSD/broken"; /usr/bin/mkfifo "$LSD/pipe"
/usr/bin/head -c 5000 /dev/zero > "$LSD/big"; /usr/bin/touch "$LSD/.dotfile"
/usr/bin/chmod 1777 "$LSD/sub/deep"; /usr/bin/chmod 4755 "$LSD/c"
i=0; for f in A.md b.txt c run.sh big pipe sub sub/z sub/deep .hidden .dotfile; do
  i=$((i+1)); /usr/bin/touch -d "2021-03-0$((i % 9 + 1)) 1$((i % 10)):00:00" "$LSD/$f"
done
/usr/bin/touch -h -d '2021-01-01 00:00' "$LSD/link" "$LSD/broken"
/usr/bin/touch -d '2021-03-03 13:00' "$LSD"
lscheck() { ( cd "$LSD" && check ls "$@" ); }
cd "$LSD"
check ls
check ls -a
check ls -A
check ls -l
check ls -la
check ls -lh
check ls -ln
check ls -lg
check ls -lo
check ls -lG
check ls -1
check ls -t
check ls -tr
check ls -S
check ls -Sr
check ls -lS
check ls -X
check ls -U -1 sub
check ls -F
check ls -p
check ls -lF
check ls -i -1 b.txt
check ls -s
check ls -sh
check ls -d sub
check ls -ld sub .
check ls -R
check ls -lR sub
check ls sub b.txt c
check ls -l link broken
check ls link
check ls nope
check ls nope b.txt
check ls -C
check ls -x
check ls -m
check ls -w 30 -C
check ls -w 30 -x
check ls --group-directories-first
check ls -l --time-style=bogus
check ls --color=always
check ls -l --color=always
check ls -lu
check ls --full-time b.txt
check ls -q
check ls -C --color=always
check ls -w 50 -C sub ..
check ls -T 0 -w 30 -C
check ls -l --time-style=long-iso
check ls -l --time-style=iso
check ls -l --time-style=full-iso b.txt
check ls -l --time-style=+%Y/%m/%d b.txt
check ls -l --time-style=posix-long-iso b.txt
check ls -w x
cd "$TMP"

# rng's own rule: removing / says "Bye" first. Only NON-recursive forms are
# run here (they just fail on the directory afterwards); never run -r on / outside a jail.
for args in "/" "//" "/." "-f /" "--no-preserve-root /"; do
  out=$(env PATH="$TMP/bin" rm $args 2>/dev/null); rc=$?
  if [ "$out" = "Bye" ] && [ "$rc" = 1 ]; then pass=$((pass+1)); else fail=$((fail+1)); echo "FAIL (bye): rm $args -> '$out' ($rc)"; fi
done

echo "passed: $pass  failed: $fail"
[ "$fail" = 0 ]
