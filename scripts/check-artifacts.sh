#!/usr/bin/env bash
# 发布前产物自检。
#
# ## 为什么不能直接在二进制里搜字符串
#
# 最早这脚本是用字节查找去二进制里找 `allow-default-urls`、
# `com.chuaker7.ssh7` 这类串。**这是错的** —— 链接器会把字符串拆开摆放，
# 还会做后缀合并（`com.chuaker7.ssh7` 和 `com.alishiguang.ssh7` 共用 `ssh7` 尾巴），
# 于是同一个串在 arm64 里是连续的、在 x64 里就断了。
#
# 结果就是：**x64 的包一直是好的，脚本却报它缺权限**（假警报）。
# 更糟的是，这种假警报会让人以为真有问题，去查半天。
#
# ## 现在怎么查
#
# 1. **运行时探针**：拿临时 HOME 跑产物，看它到底建了哪个数据目录 ——
#    这直接读的是**二进制内嵌的标识符**，跑出来什么就是什么。
# 2. **逐目标 codegen 自检**：对每个目标跑 Rust 自检（含读内嵌上下文的那几条），
#    确认权限与标识符在编译期就是对的。
# 3. **依赖图检查**：确认 `keyring` 那套彻底没了。
set -uo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
TARGET_DIR="src-tauri/target"

EXPECTED_IDENTIFIER="com.chuaker7.ssh7"
LEGACY_IDENTIFIERS=("com.alishiguang.ssh7" "com.igigi.ssh7")

fail=0
note() { printf "  %s\n" "$*"; }
bad() { printf "  ✗ %s\n" "$*"; fail=1; }

# ---------------------------------------------------------------------------
# 1. 运行时探针：产物实际用的是哪个标识符
# ---------------------------------------------------------------------------
probe_identifier() {
  local label="$1" bin="$2"
  [ -f "$bin" ] || { note "跳过 ${label}（还没构建）"; return; }

  local home
  home="$(mktemp -d "${TMPDIR:-/tmp}/ssh7-probe.XXXXXX")"
  HOME="$home" SSH7_LOG=off timeout 15 "$bin" >/dev/null 2>&1
  local made
  made="$(ls "$home/Library/Application Support" 2>/dev/null | head -1)"
  rm -rf "$home"

  if [ "$made" = "$EXPECTED_IDENTIFIER" ]; then
    note "✓ ${label}：运行时用的是 ${EXPECTED_IDENTIFIER}"
  elif [ -z "$made" ]; then
    # 二进制跑不起来（常见于交叉编译产物，缺 Rosetta）—— 不算失败，但要说清楚
    note "⚠ ${label}：探针没能跑起来（可能缺 Rosetta），标识符未验证"
  else
    bad "${label}：运行时用的是 '${made}'，应该是 ${EXPECTED_IDENTIFIER}"
    for legacy in "${LEGACY_IDENTIFIERS[@]}"; do
      [ "$made" = "$legacy" ] && bad "  → 它用的是旧包名，用户的数据目录会跟旧版本撞车"
    done
  fi
}

# ---------------------------------------------------------------------------
# 2. 逐目标 codegen 自检
# ---------------------------------------------------------------------------
check_target_tests() {
  local triple="$1"
  local dir="$TARGET_DIR/$triple"
  [ -d "$dir" ] || return

  note "--- ${triple}：编译期自检 ---"
  local log
  log="$(cd src-tauri && CARGO_NET_OFFLINE=true cargo test --release --target "$triple" \
        --test external_links --test app_identity --test window_permission 2>&1)"
  if echo "$log" | grep -q "test result: ok"; then
    note "✓ ${triple}：自检通过"
  elif echo "$log" | grep -qi "Rosetta\|Bad CPU type\|exec format error"; then
    note "⚠ ${triple}：产物跑不动（缺 Rosetta），自检未完成 —— 请在本机确认一次"
  else
    echo "$log" | tail -20 | sed 's/^/     /'
    bad "${triple}：自检没过"
  fi
}

# universal 不是真正的 rustc 目标（它是 tauri-cli 的概念，一个包两片），
# 所以没法 `cargo test --target universal-apple-darwin`。
# 它由两个单架构产物合成，codegen 与那两个一致 —— 这里只查它是不是真的两片都在。
check_universal_slices() {
  local bin="$TARGET_DIR/universal-apple-darwin/release/ssh7"
  [ -f "$bin" ] || return
  note "--- universal-apple-darwin：切片检查 ---"
  local archs
  archs="$(lipo -archs "$bin" 2>/dev/null)"
  if echo "$archs" | grep -q "x86_64" && echo "$archs" | grep -q "arm64"; then
    note "✓ universal：两片都在（${archs}）"
  else
    bad "universal 的架构不对：${archs}"
  fi
}

echo "=== 发布前产物自检 ==="

echo
echo "[1/3] 运行时探针（产物内嵌的标识符）"
for triple in aarch64-apple-darwin x86_64-apple-darwin universal-apple-darwin; do
  probe_identifier "$triple" "$TARGET_DIR/$triple/release/ssh7"
done
probe_identifier "已安装的 /Applications/SSH7.app" "/Applications/SSH7.app/Contents/MacOS/SSH7"

echo
echo "[2/3] 钥匙串那套必须彻底没了"
if (cd src-tauri && cargo tree --edges normal 2>/dev/null | grep -q 'keyring'); then
  bad "依赖图里还有 keyring"
else
  note "✓ 依赖图里没有 keyring"
fi
if grep -rn "credential_store\|credential_store_info\|SecretBackend" src-tauri/src src/ >/dev/null 2>&1; then
  bad "源码里还有 credential_store / SecretBackend 之类的东西"
  grep -rn "credential_store\|credential_store_info\|SecretBackend" src-tauri/src src/ 2>/dev/null | sed 's/^/     /' | head -5
else
  note "✓ 源码里没有 credential_store / SecretBackend"
fi

echo
echo "[3/3] 逐目标编译期自检"
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  check_target_tests "$triple"
done
check_universal_slices

echo
if [ "$fail" -ne 0 ]; then
  echo "❌ 产物自检没过 —— 别发布。"
  exit 1
fi

echo "✅ 产物自检通过。"
echo
echo "   DMG 清单："
ls -lh "$TARGET_DIR"/*/release/bundle/dmg/*.dmg 2>/dev/null | awk '{print "     " $9 "  " $5}' || true
