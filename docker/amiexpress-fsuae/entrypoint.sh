#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_ARCHIVE=/opt/amiexpress/source/AmiExpress.lha
readonly RUNTIME_DIR=/run/amiexpress-fsuae

TELNET_PORT="${TELNET_PORT:-6023}"
NODE_COUNT="${NODE_COUNT:-4}"
# DoS connection-throttling window in seconds (ACP DOSCHECKTIME). AmiExpress
# defaults to 60s with a 5-connection trigger and a 60-minute ban. Behind
# Docker NAT every host connection shares one source IP, so any concurrency
# trips that ban; default to 0 (disabled) for this localhost reference harness.
DOSCHECKTIME="${DOSCHECKTIME:-0}"
KICKSTART_FILE="${KICKSTART_FILE:-/roms/kick.rom}"
KICKSTART_EXT_FILE="${KICKSTART_EXT_FILE:-}"
WORKBENCH_DIR="${WORKBENCH_DIR:-/amiga/workbench}"
BBS_DIR="${BBS_DIR:-/amiga/bbs}"
AMIEXPRESS_ARCHIVE="${AMIEXPRESS_ARCHIVE:-}"
AMIEXPRESS_DOWNLOAD_URL="${AMIEXPRESS_DOWNLOAD_URL:-}"
BOOTSTRAP_AROS="${BOOTSTRAP_AROS:-auto}"
AROS_DIR="${AROS_DIR:-/opt/aros}"
AROS_DOWNLOAD_INDEX="${AROS_DOWNLOAD_INDEX:-https://aros.sourceforge.io/cgi-bin/files?lang=en&type=nightly2}"
AROS_BOOT_ISO_URL="${AROS_BOOT_ISO_URL:-auto}"
INSTALL_TO_WORKBENCH="${INSTALL_TO_WORKBENCH:-1}"
AUTO_CONFIGURE="${AUTO_CONFIGURE:-1}"
AUTO_START="${AUTO_START:-1}"
ALLOW_INCOMPLETE="${ALLOW_INCOMPLETE:-0}"
SEED_SYSOP="${SEED_SYSOP:-1}"
SYSOP_USERNAME="${SYSOP_USERNAME:-sysop}"
SYSOP_PASSWORD="${SYSOP_PASSWORD:-sysop}"
RESET_SYSOP="${RESET_SYSOP:-0}"
DRY_RUN="${DRY_RUN:-0}"

fail() {
    echo "amiexpress-fsuae: $*" >&2
    exit 1
}

find_case_insensitive() {
    local base=$1
    local name=$2

    [[ -d "$base" ]] || return 0
    find "$base" -maxdepth 1 -iname "$name" -print -quit 2>/dev/null || true
}

copy_missing_tree() {
    local src=$1
    local dest=$2

    if [[ -d "$src" ]]; then
        mkdir -p "$dest"
        cp -a --update=none "$src"/. "$dest"/
    fi
}

copy_missing_file() {
    local src=$1
    local dest_dir=$2

    if [[ -f "$src" ]]; then
        mkdir -p "$dest_dir"
        cp -a --update=none "$src" "$dest_dir"/
    fi
}

copy_missing_named_file() {
    local src_dir=$1
    local name=$2
    local dest_dir=$3
    local src

    src=$(find_case_insensitive "$src_dir" "$name")
    copy_missing_file "$src" "$dest_dir"
}

resolve_archive() {
    if [[ -n "$AMIEXPRESS_ARCHIVE" ]]; then
        [[ -f "$AMIEXPRESS_ARCHIVE" ]] || fail "AMIEXPRESS_ARCHIVE does not exist: $AMIEXPRESS_ARCHIVE"
        echo "$AMIEXPRESS_ARCHIVE"
        return
    fi

    if [[ -f /archive/AmiExpress.lha ]]; then
        echo /archive/AmiExpress.lha
        return
    fi

    if [[ -f /archive/Amix560.lha ]]; then
        echo /archive/Amix560.lha
        return
    fi

    if [[ -n "$AMIEXPRESS_DOWNLOAD_URL" ]]; then
        mkdir -p "$RUNTIME_DIR"
        curl -fsSL "$AMIEXPRESS_DOWNLOAD_URL" -o "$RUNTIME_DIR/AmiExpress.lha"
        echo "$RUNTIME_DIR/AmiExpress.lha"
        return
    fi

    [[ -f "$DEFAULT_ARCHIVE" ]] || fail "No AmiExpress archive is available"
    echo "$DEFAULT_ARCHIVE"
}

extract_archive() {
    local archive=$1
    local out_dir=$2

    mkdir -p "$out_dir"
    (
        cd "$out_dir"
        lha x "$archive" >/dev/null
    )
}

workbench_is_bootable() {
    [[ -f "$WORKBENCH_DIR/S/Startup-Sequence" ]]
}

resolve_aros_boot_iso_url() {
    local url

    if [[ "$AROS_BOOT_ISO_URL" != "auto" ]]; then
        echo "$AROS_BOOT_ISO_URL"
        return
    fi

    url=$(curl -fsSL "$AROS_DOWNLOAD_INDEX" \
        | sed -n 's/.*href="\([^"]*amiga-m68k-boot-iso\.zip\/download\)".*/\1/p' \
        | head -1)
    [[ -n "$url" ]] || fail "Could not resolve the latest AROS amiga-m68k boot ISO URL from $AROS_DOWNLOAD_INDEX"
    echo "$url"
}

install_aros_runtime() {
    local zip_path=$AROS_DIR/aros-amiga-m68k-boot-iso.zip
    local unpack_dir=$RUNTIME_DIR/aros
    local iso_path=
    local url=

    mkdir -p "$AROS_DIR" "$WORKBENCH_DIR" "$unpack_dir"

    if [[ ! -f "$AROS_DIR/aros-rom.bin" || ! -f "$AROS_DIR/aros-ext.bin" || ! workbench_is_bootable ]]; then
        url=$(resolve_aros_boot_iso_url)
        echo "amiexpress-fsuae: downloading AROS m68k boot ISO from $url"
        curl -fL --retry 3 -o "$zip_path" "$url"

        rm -rf "$unpack_dir"
        mkdir -p "$unpack_dir"
        unzip -q "$zip_path" -d "$unpack_dir"
        iso_path=$(find "$unpack_dir" -name '*.iso' -print -quit)
        [[ -n "$iso_path" ]] || fail "AROS boot ISO ZIP did not contain an ISO"

        bsdtar -xOf "$iso_path" boot/amiga/aros-rom.bin >"$AROS_DIR/aros-rom.bin"
        bsdtar -xOf "$iso_path" boot/amiga/aros-ext.bin >"$AROS_DIR/aros-ext.bin"

        if ! workbench_is_bootable; then
            bsdtar -xf "$iso_path" -C "$WORKBENCH_DIR"
        fi
    fi

    KICKSTART_FILE="$AROS_DIR/aros-rom.bin"
    KICKSTART_EXT_FILE="$AROS_DIR/aros-ext.bin"
}

maybe_bootstrap_aros() {
    if [[ "$BOOTSTRAP_AROS" == "0" ]]; then
        return
    fi

    if [[ "$BOOTSTRAP_AROS" == "1" ]] || [[ ! -f "$KICKSTART_FILE" ]] || ! workbench_is_bootable; then
        install_aros_runtime
    fi
}

install_amiexpress() {
    local archive=$1
    local extract_dir=$RUNTIME_DIR/archive
    local release_root=

    if [[ -n "$(find_case_insensitive "$BBS_DIR" acp)" && -n "$(find_case_insensitive "$BBS_DIR" express)" ]]; then
        return
    fi

    rm -rf "$extract_dir"
    extract_archive "$archive" "$extract_dir"

    if [[ -d "$extract_dir/AmiExpress/AmiExpress" ]]; then
        release_root="$extract_dir/AmiExpress"
        copy_missing_tree "$release_root/AmiExpress" "$BBS_DIR"
    elif [[ -d "$extract_dir/AmiExpress" ]]; then
        release_root="$extract_dir"
        copy_missing_tree "$release_root/AmiExpress" "$BBS_DIR"
    else
        release_root="$extract_dir"
    fi

    copy_missing_tree "$release_root/defaultbbs" "$BBS_DIR"
    copy_missing_tree "$release_root/AmiExpress/defaultbbs" "$BBS_DIR"
    copy_missing_tree "$release_root/AmiExpress/Storage" "$BBS_DIR/Storage"
    copy_missing_tree "$release_root/AmiExpress/Utils" "$BBS_DIR/Utils"

    if [[ "$INSTALL_TO_WORKBENCH" == "1" ]]; then
        copy_missing_named_file "$release_root/libs" aedoor.library "$WORKBENCH_DIR/Libs"
        copy_missing_named_file "$release_root/c" LhA "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/c" LZX "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/c" UnZip "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/c" Zip "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/l" LZX.keyfile "$WORKBENCH_DIR/L"
        copy_missing_named_file "$release_root/fileid" Textract "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/fileid" DMSDescript "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/fileid" EXEDescript "$WORKBENCH_DIR/C"
        copy_missing_named_file "$release_root/fileid" GIFDesc "$WORKBENCH_DIR/C"
    fi

    if [[ -z "$(find_case_insensitive "$BBS_DIR" acp)" || -z "$(find_case_insensitive "$BBS_DIR" express)" ]]; then
        if [[ "$ALLOW_INCOMPLETE" == "1" ]]; then
            echo "amiexpress-fsuae: archive did not install acp and express; continuing because ALLOW_INCOMPLETE=1" >&2
        else
            fail "archive did not contain the compiled AmiExpress acp and express binaries"
        fi
    fi
}

# Emit the JSON for a single `bbs:NodeN` config object (no leading comma).
# Every node is telnet-enabled (`TELNET`) and idle-started (`IDLENODE`) so the
# StartAmiExpress script owns process spawning; ACP just routes accepted telnet
# sockets to whichever node is awaiting a connection.
node_config_block() {
    local i=$1

    cat <<EOF
  "bbs:Node${i}": {
    "NODESTART": "BBS:express",
    "PRIORITY": 0,
    "CAPITOL_FILES": null,
    "SYSOP_CHAT_COLOR": 33,
    "USER_CHAT_COLOR": 32,
    "KEEP_UPLOAD_CREDIT": 1,
    "SCREENS": "BBS:Node${i}/Screens/",
    "FREE_RESUMING": null,
    "HDTRANSBUFFER": 1,
    "RINGCOUNT": 1,
    "CALLERS_LOG": null,
    "DEBUG_LOG": null,
    "DEF_SCREENS": null,
    "EXPFONT": "topaz.font",
    "IDLENODE": null,
    "TELNET": null,
    "CONSOLE_DEBUG": 3,
    "NO_EMAILS": null,
    "NO_MCI_MSG": null,
    "BGFILECHECK": null,
    "SERIAL": {
      "LOCAL_UNIT": {
        "SERIAL.UNIT": 0,
        "SERIAL.BAUD": 115200,
        "SERIAL_DEVICE": "serial.device"
      }
    },
    "WORK": {},
    "TIMES.DEF": {
      "START.9600": 0,
      "END.9600": 2359,
      "START.12000": 0,
      "END.12000": 2359,
      "START.14400": 0,
      "END.14400": 2359,
      "START.16800": 0,
      "END.16800": 2359,
      "START.19200": 0,
      "END.19200": 2359,
      "START.21600": 0,
      "END.21600": 2359,
      "START.24000": 0,
      "END.24000": 2359,
      "START.26400": 0,
      "END.26400": 2359,
      "START.28800": 0,
      "END.28800": 2359,
      "START.31200": 0,
      "END.31200": 2359,
      "START.33600": 0,
      "END.33600": 2359,
      "START.38400": 0,
      "END.38400": 2359,
      "START.57600": 0,
      "END.57600": 2359,
      "START.115200": 0,
      "END.115200": 2359
    },
    "WINDOW.DEF": {
      "WINDOW.NUM_COLORS": 8,
      "WINDOW.LEFTEDGE": 0,
      "WINDOW.TOPEDGE": 0,
      "WINDOW.WIDTH": 640,
      "WINDOW.HEIGHT": 256,
      "WINDOW.ICONIFIED": null
    }
  }
EOF
}

write_amiexpress_config() {
    local config_file=$BBS_DIR/docker/aeicon-docker.json
    local i

    mkdir -p "$BBS_DIR/docker"
    {
        cat <<EOF
{
  "acp": {
    "BBS_NAME": "NextExpress Reference",
    "BBS_STACK": 65535,
    "BBS_LOCATION": "BBS:",
    "BBS_GEOGRAPHIC": "Docker",
    "NODES": ${NODE_COUNT},
    "SYSOP_NAME": "${SYSOP_USERNAME}",
    "PRIORITY": 2,
    "DONOTWAIT": null,
    "ICONIFIED": null,
    "MULTICOM_PORT": null,
    "NEW_ACCOUNTS": "APPEND",
    "LANGUAGE_BASE": "bbs:languages",
    "DOSCHECKTIME": ${DOSCHECKTIME},
    "TELNETPORT": ${TELNET_PORT}
  },
EOF
        for ((i = 0; i < NODE_COUNT; i++)); do
            node_config_block "$i"
            if ((i < NODE_COUNT - 1)); then
                echo "  ,"
            fi
        done
        echo "}"
    } >"$config_file"
}

write_amiga_startup() {
    local startup=$BBS_DIR/docker/StartAmiExpress
    local i

    mkdir -p "$BBS_DIR/docker"
    for ((i = 0; i < NODE_COUNT; i++)); do
        mkdir -p "$BBS_DIR/Node${i}/Screens"
    done

    {
        cat <<'EOF'
Assign Doors: BBS:Doors
Path BBS: ADD
Path BBS:Utils ADD

If EXISTS SYS:RexxC/RX
  Resident SYS:RexxC/RX PURE
EndIf

If EXISTS BBS:Utils/jsonImport
  BBS:Utils/jsonImport CONFIG=BBS:docker/aeicon-docker.json WRITEPATH=BBS:
EndIf

If EXISTS BBS:acp
  Run >NIL: BBS:acp
Else
  Echo "BBS:acp was not found"
EndIf

Wait 5 SECS

EOF
        # Each node runs its own `express N` idle process; ACP dispatches an
        # accepted telnet socket to whichever node is awaiting a connection.
        for ((i = 0; i < NODE_COUNT; i++)); do
            cat <<EOF
If EXISTS BBS:express
  Run >NIL: BBS:express ${i}
Else
  Echo "BBS:express was not found"
EndIf
EOF
        done
    } >"$startup"
}

append_workbench_startup() {
    local user_startup=$WORKBENCH_DIR/S/User-Startup
    local marker='; NextExpress AmiExpress FS-UAE bootstrap'

    mkdir -p "$WORKBENCH_DIR/S"
    touch "$user_startup"

    if ! grep -Fq "$marker" "$user_startup"; then
        cat >>"$user_startup" <<'EOF'

; NextExpress AmiExpress FS-UAE bootstrap
If EXISTS BBS:docker/StartAmiExpress
  Execute BBS:docker/StartAmiExpress
EndIf
EOF
    fi
}

write_fsuae_config() {
    local config_path=$1

    cat >"$config_path" <<EOF
amiga_model = A4000/040
kickstart_file = ${KICKSTART_FILE}
hard_drive_0 = ${WORKBENCH_DIR}
hard_drive_0_label = System
hard_drive_0_priority = 10
hard_drive_1 = ${BBS_DIR}
hard_drive_1_label = BBS
hard_drive_1_priority = 0
bsdsocket_library = 1
kickstart_setup = 0
chip_memory = 2048
zorro_iii_memory = 65536
fullscreen = 0
automatic_input_grab = 0
EOF

    if [[ -n "$KICKSTART_EXT_FILE" ]]; then
        {
            echo "kickstart_ext_file = ${KICKSTART_EXT_FILE}"
        } >>"$config_path"
    fi
}

main() {
    [[ "$TELNET_PORT" =~ ^[0-9]+$ ]] || fail "TELNET_PORT must be numeric"
    [[ "$NODE_COUNT" =~ ^[0-9]+$ ]] && ((NODE_COUNT >= 1)) || fail "NODE_COUNT must be a positive integer"

    mkdir -p "$RUNTIME_DIR" "$BBS_DIR" "$WORKBENCH_DIR"
    maybe_bootstrap_aros

    [[ -f "$KICKSTART_FILE" ]] || fail "Kickstart ROM not found at $KICKSTART_FILE; mount one at /roms/kick.rom, set KICKSTART_FILE, or leave BOOTSTRAP_AROS=auto"
    workbench_is_bootable || fail "Workbench directory is not bootable at $WORKBENCH_DIR; mount a bootable AmigaOS directory hard drive or leave BOOTSTRAP_AROS=auto"

    local archive
    archive=$(resolve_archive)
    install_amiexpress "$archive"

    if [[ "$SEED_SYSOP" == "1" ]]; then
        local reset_arg=()
        if [[ "$RESET_SYSOP" == "1" ]]; then
            reset_arg=(--reset)
        fi
        python3 /usr/local/lib/amiexpress/seed_sysop.py "$BBS_DIR" \
            --username "$SYSOP_USERNAME" \
            --password "$SYSOP_PASSWORD" \
            "${reset_arg[@]}"
    fi

    if [[ "$AUTO_CONFIGURE" == "1" ]]; then
        write_amiexpress_config
        write_amiga_startup
    fi

    if [[ "$AUTO_START" == "1" ]]; then
        append_workbench_startup
    fi

    local fsuae_config=$RUNTIME_DIR/amiexpress.fs-uae
    write_fsuae_config "$fsuae_config"

    if [[ "$DRY_RUN" == "1" ]]; then
        echo "amiexpress-fsuae: dry run complete; AmiExpress files and FS-UAE config were prepared"
        return
    fi

    mkdir -p /tmp/runtime-amiga
    chmod 700 /tmp/runtime-amiga
    export XDG_RUNTIME_DIR=/tmp/runtime-amiga
    export SDL_AUDIODRIVER=dummy

    echo "amiexpress-fsuae: starting FS-UAE; publish TCP ${TELNET_PORT} from the container with docker run -p"
    exec xvfb-run -a --server-args="-screen 0 1024x768x24" fs-uae "$fsuae_config"
}

main "$@"
