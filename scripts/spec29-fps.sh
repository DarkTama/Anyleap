#!/usr/bin/env bash
# SPEC-29 §10 FPS acceptance. Detection off vs on, Mirror with --turn-screen-off,
# 60 s per run, first 5 s discarded, alternating off/on x3. FPS source: scrcpy --print-fps.
# Detection "on" = the real run_poll loop (examples/secure_harness) against the same phone.
# usage: spec29-fps.sh <serial> [secs=60] [runs=3] [hogs=0]
# hogs>0 starts that many CPU busy loops on the phone (PIDs in hogs.txt) and ALWAYS kills them (trap).
set -u
SERIAL=$1; SECS=${2:-60}; RUNS=${3:-3}; HOGS=${4:-0}
BIN="$(cd "$(dirname "$0")/../src-tauri/binaries" && pwd)"
export PATH="$BIN/dll:$BIN:$PATH"
HARNESS=${HARNESS:-/d/cargo-target/spec29/debug/examples/secure_harness.exe}
OUT=${OUT:-/d/cargo-target/tmp/spec29-fps}; mkdir -p "$OUT"; HOGFILE="$OUT/hogs.txt"; : > "$HOGFILE"
A="adb -s $SERIAL"

restore() {
  while read -r p; do [ -n "$p" ] && $A shell "kill $p" >/dev/null 2>&1; done < "$HOGFILE"
  : > "$HOGFILE"
}
trap restore EXIT INT TERM

if [ "$HOGS" -gt 0 ]; then
  for i in $(seq "$HOGS"); do
    $A shell "sh -c 'while :; do :; done' >/dev/null 2>&1 & echo \$!" | tr -d '\r' >> "$HOGFILE"
  done
  sleep 2
  for i in 1 2 3; do
    s=$(date +%s%N); $A shell "dumpsys window | grep -m1 mCurrentFocus" >/dev/null; e=$(date +%s%N)
    echo "hog probe $(( (e-s)/1000000 )) ms"
  done
fi

run() { # arm n
  local arm=$1 n=$2; local f="$OUT/$arm-$n"
  $A shell am start -a android.intent.action.VIEW -d https://www.testufo.com/ com.android.chrome >/dev/null 2>&1
  local hp=0
  scrcpy -s "$SERIAL" --turn-screen-off --no-audio --print-fps > "$f.scrcpy" 2>&1 &
  local sp=$!
  sleep 3
  if [ "$arm" = on ]; then
    "$HARNESS" "$BIN/adb.exe" "$SERIAL" mirror "$((SECS-2))" 0 0 1 > "$f.harness" 2>&1 &
    hp=$!
  fi
  # keepalive (both arms): wake key only, so the phone does not time out and lock mid-run
  ( while :; do sleep 25; $A shell input keyevent KEYCODE_WAKEUP >/dev/null 2>&1; done ) &
  local kp=$!
  sleep "$SECS"
  kill "$kp" 2>/dev/null
  [ "$hp" != 0 ] && wait "$hp" 2>/dev/null
  kill "$sp" 2>/dev/null; taskkill //F //IM scrcpy.exe >/dev/null 2>&1; wait "$sp" 2>/dev/null
  # drop 5 s warm-up, keep SECS-5 samples
  grep -o 'INFO: [0-9]* fps' "$f.scrcpy" | awk '{print $2}' | tail -n +6 > "$f.fps"
  awk -v arm="$arm" -v n="$n" '{s+=$1; if(NR==1||$1<m)m=$1} END{printf "%s run%s: n=%d mean=%.2f min=%d\n",arm,n,NR,(NR?s/NR:0),m}' "$f.fps"
}

for n in $(seq "$RUNS"); do run off "$n"; run on "$n"; done | tee "$OUT/summary.txt"
awk '{split($4,a,"=");split($5,b,"="); if($1=="off"){os+=a[2];oc++; if(om==""||b[2]<om)om=b[2]} else {ns+=a[2];nc++; if(nm==""||b[2]<nm)nm=b[2]}} END{printf "MEAN off=%.2f on=%.2f drop=%.2f%% (%.2f fps)  MIN off=%d on=%d\n",os/oc,ns/nc,(os/oc-ns/nc)/(os/oc)*100,os/oc-ns/nc,om,nm}' "$OUT/summary.txt"
for f in "$OUT"/on-*.harness; do echo "$f: $(grep TICK "$f" | tail -1) $(grep -c 'phone too slow' "$f") autodisable"; done
