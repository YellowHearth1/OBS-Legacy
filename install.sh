#!/usr/bin/env bash
# =============================================================================
#  install.sh — встановлює TUI OBS-Legacy для поточного користувача
#  https://github.com/YellowHearth1/OBS-Legacy
# -----------------------------------------------------------------------------
#  Створено за допомогою Claude Code (ШІ-асистент Anthropic) на замовлення і
#  під керівництвом автора репозиторію. Натхнення:
#  https://github.com/sl5net/build-obs-nvenc-legacy
#
#  Що робить:
#    * збирає obs-legacy (cargo build --release --locked), а після встановлення
#      прибирає проміжні файли збирання (target/, сотні мегабайтів);
#    * кладе його в ~/.local/bin, додає ярлик «OBS-Legacy» у меню програм та
#      іконку, за бажанням — ярлик на робочий стіл. Клацання по ярлику одразу
#      відкриває вікно термінала з меню OBS-Legacy;
#    * за бажанням вмикає сповіщення про нові версії OBS, плагінів і самого
#      OBS-Legacy: службу користувача systemd, dinit, OpenRC, runit чи s6 або
#      автозапуск сесії (obs-legacy --notifier, без sudo).
#  У систему нічого не ставить, крім (лише з твоєї згоди) пакета rust.
#
#  Використання:
#    ./install.sh              встановити або оновити (запитає про ярлик на столі)
#    ./install.sh --desktop    те саме + ярлик на робочому столі без запитання
#    ./install.sh --no-desktop без ярлика на робочому столі
#    ./install.sh --update     без запитань (так його викликає сам obs-legacy)
#    ./install.sh --uninstall  прибрати TUI і сповіщення (зібраний OBS не чіпає)
# =============================================================================
set -Eeuo pipefail

SRC_DIR=$(cd -- "$(dirname -- "$(readlink -f -- "${BASH_SOURCE[0]}")")" && pwd)
# Явно, а не з CARGO_TARGET_DIR: звідси беремо бінарник і цю теку потім прибираємо.
TARGET_DIR="$SRC_DIR/target"
BIN_DIR="$HOME/.local/bin"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/scalable/apps"
STATE_DIR="$DATA_HOME/obs-legacy"
DESKTOP_ID="obs-legacy.desktop"
readonly MARKER="Managed by OBS-Legacy install.sh"

if [[ -t 2 ]]; then
  C_RED=$'\e[31m' C_GRN=$'\e[32m' C_YEL=$'\e[33m' C_BLU=$'\e[34m' C_BLD=$'\e[1m' C_RST=$'\e[0m'
else
  C_RED='' C_GRN='' C_YEL='' C_BLU='' C_BLD='' C_RST=''
fi
say()  { printf '%s==>%s %s\n' "$C_BLU$C_BLD" "$C_RST" "$*" >&2; }
ok()   { printf '%s[OK]%s %s\n' "$C_GRN" "$C_RST" "$*" >&2; }
warn() { printf '%s[УВАГА]%s %s\n' "$C_YEL" "$C_RST" "$*" >&2; }
die()  { printf '%s[ПОМИЛКА]%s %s\n' "$C_RED" "$C_RST" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

INTERACTIVE=1
[[ -t 0 ]] || INTERACTIVE=0

# Відповідь без пробілів і керівних символів (коди стрілок, Esc). LC_ALL=C: в
# українській локалі діапазон [@-~] не охоплює латинських літер.
clean_answer() {
  local esc=$'\e'
  printf '%s' "$1" | LC_ALL=C sed -E "s|${esc}\\[[0-9;?]*[ -/]*[@-~]||g; s|${esc}O.||g; s|[[:cntrl:]]||g; s|[[:space:]]||g"
}

# Питання «так / ні». Незрозуміла відповідь — питання ще раз. «т» і «н» неоднозначні
# («так» / «ні», але й клавіші N і Y в українській розкладці) — підказка й ще раз.
ask_yes_no() {   # $1 = питання, $2 = «[y/N]» чи «[Y/n]», $3 = відповідь на Enter (0 — так, 1 — ні)
  local answer a
  (( INTERACTIVE )) || return 1
  while :; do
    printf '%s %s: ' "$1" "$2" >&2
    read -r answer || return 1
    a=$(clean_answer "$answer")
    case "$a" in
      '') return "$3" ;;
      [yY]|[yY][eE][sS]|так|Так|ТАК) return 0 ;;
      [nN]|[nN][oO]|ні|Ні|НІ) return 1 ;;
      т|Т|н|Н)
        printf '  «%s» — схоже, увімкнена українська розкладка (клавіша Y дає «н», N — «т»). Введи y чи n латиницею або напиши «так» / «ні».\n' "$a" >&2 ;;
      *) printf '  Не зрозумів відповідь «%s». Введи y (так) або n (ні).\n' "$a" >&2 ;;
    esac
  done
}

confirm() { ask_yes_no "$1" '[y/N]' 1; }       # Enter — ні. Без термінала — ні.
confirm_yes() { ask_yes_no "$1" '[Y/n]' 0; }   # Enter — так. Без термінала — ні.

# Атомарна заміна файлу: працює навіть тоді, коли obs-legacy зараз запущений.
put() {   # $1 = джерело, $2 = призначення, $3 = права
  local tmp
  mkdir -p -- "$(dirname -- "$2")"
  tmp=$(mktemp -- "$2.XXXXXX")
  cp -- "$1" "$tmp"
  chmod "$3" -- "$tmp"
  mv -f -- "$tmp" "$2"
}

desktop_dir() {
  local d=''
  if have xdg-user-dir; then d=$(xdg-user-dir DESKTOP 2>/dev/null || true); fi
  if [[ -z "$d" || "$d" == "$HOME" ]]; then d="$HOME/Desktop"; fi
  printf '%s\n' "$d"
}

write_desktop_entry() {   # $1 = файл
  local exec_path="$BIN_DIR/obs-legacy"
  if [[ "$exec_path" == *' '* ]]; then exec_path="\"$exec_path\""; fi
  cat > "$1" <<EOF
[Desktop Entry]
# $MARKER
Type=Application
Name=OBS-Legacy
GenericName=OBS Studio з NVENC для NVIDIA 580xx
GenericName[en]=OBS Studio with NVENC for NVIDIA 580xx
Comment=Збирання, встановлення й оновлення OBS Studio з NVENC під драйвери NVIDIA 580xx
Comment[en]=Build, install and update OBS Studio with NVENC for NVIDIA 580xx legacy drivers
Exec=$exec_path
Icon=obs-legacy
Terminal=false
Categories=AudioVideo;Recorder;
Keywords=OBS;NVENC;NVIDIA;580;stream;record;
StartupNotify=false
EOF
}

# Сповіщення про оновлення: після оновлення програми — перезапустити фонову
# перевірку з новою версією; під час встановлення — запропонувати ввімкнути.
setup_notifier() {   # $1 = update|ask
  local bin="$BIN_DIR/obs-legacy"
  if "$bin" --notifier is-enabled; then
    "$bin" --notifier restart || true
    return 0
  fi
  [[ "$1" == ask ]] || return 0
  printf '\n' >&2
  say "Сповіщення про оновлення"
  printf '  Раз на 12 годин OBS-Legacy у фоні перевірятиме, чи вийшли нові OBS, плагіни чи сама програма,\n' >&2
  printf '  а після оновлень системи — чи власному OBS не бракує бібліотек. Без sudo; вимкнути можна в меню.\n' >&2
  if confirm_yes "Увімкнути сповіщення?"; then
    "$bin" --notifier enable || warn "Не вдалося ввімкнути сповіщення — спробуй пізніше: obs-legacy → Додатково → «Сповіщення про оновлення»."
  else
    printf '  Добре. Увімкнути пізніше: obs-legacy → Додатково → «Сповіщення про оновлення».\n' >&2
  fi
}

banner() {
  printf '\n%sOBS-Legacy%s — OBS Studio з NVENC для NVIDIA 580xx (Artix / Arch)\n' "$C_GRN$C_BLD" "$C_RST" >&2
  printf '%sСкрипт зроблений за допомогою Claude Code (Anthropic).%s\n' "$C_YEL$C_BLD" "$C_RST" >&2
  printf 'Натхнення: https://github.com/sl5net/build-obs-nvenc-legacy — дякуємо!\n\n' >&2
}

ensure_rust() {
  local priv=''
  if have cargo; then return 0; fi
  if have rustup; then
    say "Є rustup, але без тулчейна — ставлю stable (rustup default stable)"
    rustup default stable
    if have cargo; then return 0; fi
  fi
  warn "Для збирання TUI потрібен Rust (cargo)."
  have pacman || die "pacman не знайдено. Встанови Rust вручну (https://rustup.rs) і повтори."
  if have sudo; then priv=sudo; elif have doas; then priv=doas; else die "Немає ні sudo, ні doas — встанови пакет rust вручну."; fi
  confirm "Встановити пакет rust з репозиторію ($priv pacman -S --needed rust)?" \
    || die "Скасовано. Без Rust зібрати OBS-Legacy не вийде."
  "$priv" pacman -S --needed rust
  have cargo || die "Після встановлення cargo так і не знайшовся."
}

# Проміжні файли cargo після встановлення не потрібні (бінарник уже скопійовано),
# а важать сотні мегабайтів. Наступне збирання піде з нуля — хвилину-дві.
clean_build_cache() {
  local size
  [[ -d "$TARGET_DIR" ]] || return 0
  size=$(du -sh -- "$TARGET_DIR" 2>/dev/null | cut -f1 || true)
  if (cd -- "$SRC_DIR" && cargo clean --quiet --target-dir "$TARGET_DIR"); then
    ok "Прибрано проміжні файли збирання Rust: $TARGET_DIR (${size:-?})"
  else
    warn "Не вдалося прибрати $TARGET_DIR — прибери вручну: cd '$SRC_DIR' && cargo clean"
  fi
}

install_desktop_shortcut() {
  local dir file
  dir=$(desktop_dir)
  if [[ ! -d "$dir" ]]; then
    warn "Теки робочого столу $dir немає — ярлик не додаю."
    return 0
  fi
  file="$dir/$DESKTOP_ID"
  if [[ -e "$file" ]] && ! grep -qs -- "$MARKER" "$file"; then
    warn "$file уже існує і створений не цим установником — не чіпаю."
    return 0
  fi
  put "$APPS_DIR/$DESKTOP_ID" "$file" 755
  # GNOME / Nautilus і Xfce вимагають позначити ярлик як довірений.
  if have gio; then
    gio set "$file" metadata::trusted true 2>/dev/null || true
    if [[ "${XDG_CURRENT_DESKTOP:-}" == *XFCE* ]]; then
      gio set -t string "$file" metadata::xfce-exe-checksum "$(sha256sum -- "$file" | awk '{print $1}')" 2>/dev/null || true
    fi
  fi
  ok "Ярлик на робочому столі: $file"
}

do_install() {   # $1 = yes|no|ask — ярлик на робочому столі
  local want_desktop=$1 tmp desktop_file
  (( EUID != 0 )) || die "Не запускай від root: TUI ставиться для звичайного користувача."
  [[ -f "$SRC_DIR/Cargo.toml" ]] || die "Не знайдено Cargo.toml поруч з install.sh ($SRC_DIR)."
  banner
  ensure_rust

  say "Збираю obs-legacy (cargo build --release --locked) — це займе хвилину-дві"
  # Як і OBS, програму збираємо під цей процесор (вона ставиться лише на нього).
  (cd -- "$SRC_DIR" && RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-cpu=native" \
     cargo build --release --locked --target-dir "$TARGET_DIR")

  say "Встановлюю"
  put "$TARGET_DIR/release/obs-legacy" "$BIN_DIR/obs-legacy" 755
  ok "Програма: $BIN_DIR/obs-legacy"
  clean_build_cache
  put "$SRC_DIR/assets/obs-legacy.svg" "$ICON_DIR/obs-legacy.svg" 644
  tmp=$(mktemp)
  write_desktop_entry "$tmp"
  put "$tmp" "$APPS_DIR/$DESKTOP_ID" 644
  rm -f -- "$tmp"
  if have update-desktop-database; then update-desktop-database -q "$APPS_DIR" 2>/dev/null || true; fi
  if have gtk-update-icon-cache && [[ -f "$DATA_HOME/icons/hicolor/index.theme" ]]; then
    gtk-update-icon-cache -q -t "$DATA_HOME/icons/hicolor" 2>/dev/null || true
  fi
  ok "Ярлик у меню програм: OBS-Legacy ($APPS_DIR/$DESKTOP_ID)"
  mkdir -p -- "$STATE_DIR"
  printf '%s\n' "$SRC_DIR" > "$STATE_DIR/source-dir"

  desktop_file="$(desktop_dir)/$DESKTOP_ID"
  case "$want_desktop" in
    yes) install_desktop_shortcut ;;
    ask)
      if grep -qs -- "$MARKER" "$desktop_file"; then
        install_desktop_shortcut
      elif confirm "Додати ярлик OBS-Legacy на робочий стіл?"; then
        install_desktop_shortcut
      fi
      ;;
    update)
      if grep -qs -- "$MARKER" "$desktop_file"; then install_desktop_shortcut; fi
      ;;
  esac

  if [[ "$want_desktop" == update ]]; then setup_notifier update; else setup_notifier ask; fi

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR немає в PATH: команда obs-legacy в терміналі не знайдеться (ярлик у меню працює). Додай у ~/.bashrc або ~/.zshrc: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
  esac

  printf '\n%sГОТОВО ✓%s\n' "$C_GRN$C_BLD" "$C_RST" >&2
  printf '  Запуск: меню програм → «OBS-Legacy» (або команда obs-legacy). Клацання відкриє термінал з меню.\n' >&2
  printf '  Оновити OBS-Legacy: у меню obs-legacy → «Оновити OBS-Legacy» (або git pull && ./install.sh).\n' >&2
  if [[ "$want_desktop" != update ]] && confirm "Відкрити OBS-Legacy зараз?"; then
    exec "$BIN_DIR/obs-legacy"
  fi
}

do_uninstall() {
  local f installed
  banner
  say "Прибираю TUI OBS-Legacy (пакети OBS-Legacy це НЕ видаляє)"
  installed=$(pacman -Qq 2>/dev/null | grep -E -- '-legacy-580xx$' | tr '\n' ' ' || true)
  if [[ -n "$installed" || -d /opt/obs-studio-nvenc ]]; then
    warn "Лишаються встановленими: ${installed:-стара збірка в /opt/obs-studio-nvenc}. Щоб прибрати і їх, спершу запусти obs-legacy → «Видалити OBS-Legacy»."
    confirm "Прибрати лише TUI?" || exit 1
  fi
  if [[ -x "$BIN_DIR/obs-legacy" ]]; then
    "$BIN_DIR/obs-legacy" --notifier disable || warn "Сповіщення не вдалося вимкнути: obs-legacy --notifier status підкаже, що лишилось."
  fi
  rm -rf -- "${XDG_STATE_HOME:-$HOME/.local/state}/obs-legacy"
  rm -f -- "$BIN_DIR/obs-legacy" "$ICON_DIR/obs-legacy.svg" "$STATE_DIR/source-dir"
  for f in "$APPS_DIR/$DESKTOP_ID" "$(desktop_dir)/$DESKTOP_ID"; do
    if grep -qs -- "$MARKER" "$f"; then rm -f -- "$f"; fi
  done
  if have update-desktop-database; then update-desktop-database -q "$APPS_DIR" 2>/dev/null || true; fi
  # Розпакований скрипт збирання потрібен, доки встановлені пакети OBS-Legacy
  # чи стара збірка в /opt: на нього посилаються підказки pacman-хука й обгортки.
  if [[ -z "$installed" && ! -d /opt/obs-studio-nvenc ]]; then
    rm -f -- "$STATE_DIR/build-obs-nvenc-legacy-artix.sh"
    rmdir -- "$STATE_DIR" 2>/dev/null || true
  fi
  ok "TUI прибрано. Налаштування лишились у ${XDG_CONFIG_HOME:-$HOME/.config}/obs-legacy/ — видали вручну, якщо не потрібні."
}

main() {
  local desktop=ask
  case "${1:-}" in
    '')           ;;
    --desktop)    desktop=yes ;;
    --no-desktop) desktop=no ;;
    --update)     desktop=update; INTERACTIVE=0 ;;
    --uninstall)  do_uninstall; return ;;
    -h|--help)    sed -n '2,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; return ;;
    *)            die "Невідомий параметр: $1 (див. ./install.sh --help)" ;;
  esac
  do_install "$desktop"
}

main "$@"
