#!/usr/bin/env bash
# 同一台机器上对比：
#   current = 生产式构建（默认 target，不带 AVX-512 codegen）
#   native  = 升级版构建（-C target-cpu=native，自动开 AVX-512）
#
# 用法：
#   bash ab-compare.sh
#   MSG_LEN=1500 bash ab-compare.sh     # 按生产 V1 消息大小测
#   N=3 ITERS=3000 ROUNDS=7 bash ab-compare.sh
#   CPU=2 bash ab-compare.sh          # 可选：pin 到某个物理核
#
# 两个 variant 来自同一份 source，唯一差异是编译 flags；
# 每个 binary 内部同时报了 dalek 2.2（现有库）和 dalek 3.0（升级库）。
set -euo pipefail

REPO=https://github.com/miaomiaowu0428/sol-tx-sender.git
BRANCH=ed25519-bench
ITERS=${ITERS:-5000}
ROUNDS=${ROUNDS:-9}
MSG_LEN=${MSG_LEN:-500}
N=${N:-5}
CPU=${CPU:-}
ROOT=${ROOT:-/tmp/ed25519-ab}

mkdir -p "$ROOT"

echo "== installing current (generic target) =="
cargo install --git "$REPO" --branch "$BRANCH" --locked --force --root "$ROOT/current"

echo "== installing native (target-cpu=native / AVX-512) =="
cargo install --git "$REPO" --branch "$BRANCH" --locked --force --root "$ROOT/native" --config 'build.rustflags=["-C","target-cpu=native"]'

echo
echo "== build flags =="
"$ROOT/current/bin/ed25519-bench" --check | grep build-time
"$ROOT/native/bin/ed25519-bench" --check | grep build-time

run_one() {
  local name=$1 i=$2
  local bin="$ROOT/$name/bin/ed25519-bench"
  if [ -n "$CPU" ]; then
    taskset -c "$CPU" "$bin" --iters "$ITERS" --rounds "$ROUNDS" --msg-len "$MSG_LEN" > "$ROOT/$name-$i.txt"
  else
    "$bin" --iters "$ITERS" --rounds "$ROUNDS" --msg-len "$MSG_LEN" > "$ROOT/$name-$i.txt"
  fi
}

echo
echo "== alternating runs: current / native =="
for i in $(seq 1 "$N"); do
  run_one current "$i"
  run_one native "$i"
  echo "  round $i done"
done

minv() { grep -h "$2" "$ROOT/$1"-*.txt | awk '{print $NF}' | sort -n | head -1; }
medv() { grep -h "$2" "$ROOT/$1"-*.txt | awk '{print $NF}' | sort -n | awk '{a[NR]=$1} END{print a[int((NR+1)/2)]}'; }

echo
echo "== summary (min over $N alternating runs, message=$MSG_LEN B, us/sig) =="
printf "%-32s %10s %10s\n" "metric" "current" "native"
printf "%-32s %10s %10s\n" "------" "-------" "------"
printf "%-32s %10s %10s\n" "solana Keypair (dalek 2.2)" "$(minv current 'solana Keypair')" "$(minv native 'solana Keypair')"
printf "%-32s %10s %10s\n" "dalek 2.2 sign" "$(minv current 'ed25519-dalek 2.2')" "$(minv native 'ed25519-dalek 2.2')"
printf "%-32s %10s %10s\n" "dalek 3.0 sign" "$(minv current 'ed25519-dalek 3.0')" "$(minv native 'ed25519-dalek 3.0')"
printf "%-32s %10s %10s\n" "ring sign" "$(minv current 'ring')" "$(minv native 'ring')"
printf "%-32s %10s %10s\n" "paired dalek3/dalek2" "$(medv current 'paired')" "$(medv native 'paired')"

echo
echo "raw outputs: $ROOT/{current,native}-*.txt"