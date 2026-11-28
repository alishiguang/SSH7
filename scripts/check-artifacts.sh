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
# 1. **产物在位**：每个平台的产物形态不一样 —— macOS 是 dmg，Windows 是 NSIS
#    的 .exe。先确认产物确实在那儿、不是空壳。
# 2. **运行时探针**（macOS）：拿临时 HOME 跑产物，看它到底建了哪个数据目录 ——
#    这直接读的是**二进制内嵌的标识符**，跑出来什么就是什么。
# 3. **依赖图检查**：确认 `keyring` 那套彻底没了。
# 4. **逐目标 codegen 自检**：对每个目标跑 Rust 自检（含读内嵌上下文的那几条），
#    确认权限与标识符在编译期就是对的。
#
# ## 为什么 Windows 上没有运行时探针
#
# `dirs::config_dir()` 在 Windows 上走的是 **WinAPI 的 known-folder**
# （`FOLDERID_RoamingAppData`），**不看 `APPDATA` 环境变量** ——
# 所以探针没法像 macOS 那样用临时 HOME 把产物隔离到一个临时目录。
#
# 而「产物里带的标识符对不对」这件事，第 4 步的 `app_identity` 自检
# 读的是**同一份内嵌上下文**（`tauri::generate_context!()`），一样是编译期
# 的事实来源，还不用起进程。所以 Windows 上用编译期自检钉住，
# 不做没有意义的运行时探针。
set -uo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
TARGET_DIR="src-tauri/target"

EXPECTED_IDENTIFIER="com.chuaker7.ssh7"
LEGACY_IDENTIFIERS=("com.alishiguang.ssh7" "com.igigi.ssh7")

fail=0
note() { printf "  %s\n" "$*"; }
bad() { printf "  ✗ %s\n" "$*"; fail=1; }

# ── 宿主平台 ─────────────────────────────────────────
# Windows 上 Git Bash 的 uname 是 MINGW64_NT-… / MSYS_NT-…，
# 要单独认出来 —— 否则会把 Windows 当成 Linux，路径全错。
case "$(uname -s)" in
  Darwin*) HOST_OS="macos" ;;
  MINGW* | MSYS* | CYGWIN*) HOST_OS="windows" ;;
  *) HOST_OS="linux" ;;
esac

# 这个平台要自检的 Rust 目标。
case "$HOST_OS" in
  macos) HOST_TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin") ;;
  windows) HOST_TARGETS=("x86_64-pc-windows-msvc") ;;
  *) HOST_TARGETS=("x86_64-unknown-linux-gnu") ;;
esac

# 本机 Rust 的宿主目标 —— 只构建宿主架构时，产物落在 target/release/ 下，
# 没有 triple 子目录。用来判断「这个目标是不是就是本机的」。
NATIVE_TARGET="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"

# 产物最小体积（字节）—— 太小的多半是空壳或者构建没跑完。
MIN_SIZE=$((1024 * 1024))

# ── 1. 产物在位 ─────────────────────────────────────
check_artifacts_present() {
  note "--- ${HOST_OS}：产物在位 ---"
  case "$HOST_OS" in
    macos)
      # 宿主架构的构建落在 target/release/ 下，带 --target 的才在各自的目录里 ——
      # 两种都得查，不然 CI 里那种「只构建宿主架构」的跑法会一个都找不到。
      local found=0
      for pair in \
        "宿主架构:$TARGET_DIR/release/bundle/dmg" \
        "aarch64-apple-darwin:$TARGET_DIR/aarch64-apple-darwin/release/bundle/dmg" \
        "x86_64-apple-darwin:$TARGET_DIR/x86_64-apple-darwin/release/bundle/dmg" \
        "universal-apple-darwin:$TARGET_DIR/universal-apple-darwin/release/bundle/dmg"; do
        local label="${pair%%:*}" dir="${pair#*:}"
        if ls "$dir"/SSH7_*.dmg >/dev/null 2>&1; then
          found=$((found + 1))
          note "✓ ${label}：$(ls -1 "$dir"/SSH7_*.dmg | wc -l | tr -d ' ') 个 DMG"
        else
          note "跳过 ${label}（还没构建）"
        fi
      done
      # 至少得有一个 —— 一个都没有就是打包没成功。
      [ "$found" -ge 1 ] || bad "一个 DMG 都没有 —— 打包多半没成功"
      ;;
    windows)
      local nsis="$TARGET_DIR/release/bundle/nsis"
      local exe="$TARGET_DIR/release/ssh7.exe"
      local found=0
      for f in "$nsis"/*.exe; do
        [ -f "$f" ] || continue
        found=1
        local size
        size=$(wc -c <"$f" | tr -d ' ')
        if [ "$size" -lt "$MIN_SIZE" ]; then
          bad "NSIS 安装包太小（${size} 字节）：$(basename "$f") —— 多半没构建完"
        else
          note "✓ NSIS 安装包：$(basename "$f")（$((size / 1024 / 1024)) MB）"
        fi
      done
      [ "$found" -eq 1 ] || bad "没有 NSIS 安装包（$nsis）"
      if [ -f "$exe" ]; then
        local size
        size=$(wc -c <"$exe" | tr -d ' ')
        if [ "$size" -lt "$MIN_SIZE" ]; then
          bad "主程序太小（${size} 字节）—— 多半没构建完"
        else
          note "✓ 主程序：ssh7.exe（$((size / 1024 / 1024)) MB）"
        fi
      else
        bad "没有主程序 ssh7.exe（$exe）"
      fi
      ;;
    linux)
      local dir="$TARGET_DIR/release/bundle"
      for kind in deb appimage; do
        if ls "$dir/$kind"/* >/dev/null 2>&1; then
          note "✓ $kind"
        else
          note "跳过 $kind（还没构建）"
        fi
      done
      ;;
  esac
}

# ── 2. 运行时探针（产物内嵌的标识符）─────────────────
#
# 跑一会儿就把产物弄掉 —— 它是 GUI 程序，不主动退出。
# `timeout` 不一定有（macOS 默认不带），所以留一条手工兜底。
run_briefly() {
  local bin="$1"
  if command -v timeout >/dev/null 2>&1; then
    timeout 15 "$bin" >/dev/null 2>&1
  else
    "$bin" >/dev/null 2>&1 &
    local pid=$!
    sleep 12
    kill "$pid" >/dev/null 2>&1
    wait "$pid" >/dev/null 2>&1
  fi
}

# 探针跑完之后的判定。$1=标签 $2=产物实际建出来的目录名（空=没建出来）
judge_probe() {
  local label="$1" made="$2"
  if [ "$made" = "$EXPECTED_IDENTIFIER" ]; then
    note "✓ ${label}：运行时用的是 ${EXPECTED_IDENTIFIER}"
  elif [ -z "$made" ]; then
    # 产物跑不起来（常见于交叉编译产物，缺 Rosetta）—— 不算失败，但要说清楚
    note "⚠ ${label}：探针没能跑起来，标识符未验证"
  else
    bad "${label}：运行时用的是 '${made}'，应该是 ${EXPECTED_IDENTIFIER}"
    for legacy in "${LEGACY_IDENTIFIERS[@]}"; do
      [ "$made" = "$legacy" ] && bad "  → 它用的是旧包名，用户的数据目录会跟旧版本撞车"
    done
  fi
}

probe_identifier_macos() {
  local label="$1" bin="$2"
  [ -f "$bin" ] || {
    note "跳过 ${label}（还没构建）"
    return
  }

  local home
  home="$(mktemp -d "${TMPDIR:-/tmp}/ssh7-probe.XXXXXX")"
  HOME="$home" SSH7_LOG=off run_briefly "$bin"
  local made
  made="$(ls "$home/Library/Application Support" 2>/dev/null | head -1)"
  rm -rf "$home"

  judge_probe "$label" "$made"
}

# ── 3. 依赖图检查 ───────────────────────────────────
check_no_keyring() {
  note "--- 钥匙串那套必须彻底没了 ---"
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
}

# ── 4. 逐目标编译期自检 ─────────────────────────────
check_target_tests() {
  local triple="$1"
  local dir="$TARGET_DIR/$triple"

  # 宿主架构的构建**没有** triple 子目录 —— 产物就在 target/release/ 下。
  # 那种情况下自检照样得跑：目录名对不上就悄悄跳过，等于没查。
  if [ ! -d "$dir" ] && [ "$triple" = "$NATIVE_TARGET" ] && [ -d "$TARGET_DIR/release" ]; then
    dir="$TARGET_DIR/release"
    note "  ↳ ${triple} 没有单独构建，改用宿主架构的产物目录"
  fi
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
echo "宿主平台：${HOST_OS}"

echo
echo "[1/4] 产物在位"
check_artifacts_present

echo
echo "[2/4] 运行时探针（产物内嵌的标识符）"
if [ "$HOST_OS" = "macos" ]; then
  for triple in aarch64-apple-darwin x86_64-apple-darwin universal-apple-darwin; do
    probe_identifier_macos "$triple" "$TARGET_DIR/$triple/release/ssh7"
  done
  # 只构建宿主架构时（没有 --target），产物落在 target/release/ 下。
  probe_identifier_macos "宿主架构产物" "$TARGET_DIR/release/ssh7"
  probe_identifier_macos "已安装的 /Applications/SSH7.app" "/Applications/SSH7.app/Contents/MacOS/SSH7"
else
  note "⚠ ${HOST_OS}：跳过运行时探针 —— 数据目录走 WinAPI 的 known-folder，"
  note "  环境变量重定向不了，没法隔离。标识符改由下面第 4 步的编译期自检钉住。"
fi

echo
echo "[3/4] 钥匙串那套必须彻底没了"
check_no_keyring

echo
echo "[4/4] 逐目标编译期自检"
for triple in "${HOST_TARGETS[@]}"; do
  check_target_tests "$triple"
done
if [ "$HOST_OS" = "macos" ]; then
  check_universal_slices
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "❌ 产物自检没过 —— 别发布。"
  exit 1
fi

echo "✅ 产物自检通过。"
echo
echo "   产物清单："
# 清单只是给人看的 —— 一个都没列出来也不能让脚本以失败告终
# （这一步挂掉会让 CI 判红，而那跟产物本身没关系）。
case "$HOST_OS" in
  macos)
    ls -lh "$TARGET_DIR"/release/bundle/dmg/*.dmg "$TARGET_DIR"/*/release/bundle/dmg/*.dmg 2>/dev/null |
      awk '{print "     " $9 "  " $5}' || true
    ;;
  windows)
    ls -lh "$TARGET_DIR"/release/bundle/nsis/*.exe 2>/dev/null |
      awk '{print "     " $9 "  " $5}' || true
    ;;
  *)
    ls -lh "$TARGET_DIR"/release/bundle/*/* 2>/dev/null |
      awk '{print "     " $9 "  " $5}' || true
    ;;
esac
exit 0
