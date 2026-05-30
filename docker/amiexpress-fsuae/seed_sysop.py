#!/usr/bin/env python3
import argparse
import hashlib
import os
import struct
import sys


PWD_PBKDF2_100 = 3
SYSOP_SECURITY_LEVEL = 255
SYSOP_ACCESS_AREA = "Sysop"
# The bundled AmiExpress config uses conference 1 for New Users ("Lamer Land")
# and conference 2 for the normal Amiga menu.
SYSOP_CONFERENCE_REJOIN = 2


class AmigaRecord:
    def __init__(self, size: int):
        self.data = bytearray(size)
        self.offset = 0

    def align_word(self) -> None:
        if self.offset % 2:
            self.offset += 1

    def bytes(self, value: bytes, size: int) -> None:
        raw = value[:size]
        self.data[self.offset:self.offset + len(raw)] = raw
        self.offset += size

    def text(self, value: str, size: int) -> None:
        self.bytes(value.encode("latin-1", "replace"), size)

    def char(self, value: int | str) -> None:
        if isinstance(value, str):
            value = ord(value)
        self.data[self.offset] = value & 0xFF
        self.offset += 1

    def int16(self, value: int) -> None:
        self.align_word()
        struct.pack_into(">h", self.data, self.offset, value)
        self.offset += 2

    def uint16(self, value: int) -> None:
        self.align_word()
        struct.pack_into(">H", self.data, self.offset, value)
        self.offset += 2

    def int32(self, value: int) -> None:
        self.align_word()
        struct.pack_into(">i", self.data, self.offset, value)
        self.offset += 4

    def uint32(self, value: int) -> None:
        self.align_word()
        struct.pack_into(">I", self.data, self.offset, value)
        self.offset += 4

    def finish(self) -> bytes:
        return bytes(self.data)


def user_record(username: str) -> bytes:
    record = AmigaRecord(232)
    record.text(username, 31)
    record.text("", 9)
    record.text("Docker", 30)
    record.text("0000000000", 13)
    record.int16(1)
    record.int16(SYSOP_SECURITY_LEVEL)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int32(0)
    record.int32(-1)
    record.int32(0)
    record.int32(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int16(0)
    record.int32(0)
    record.int16(0)
    record.int16(0)
    record.text(SYSOP_ACCESS_AREA, 10)
    record.int16(0)
    record.int16(0)
    record.int16(SYSOP_CONFERENCE_REJOIN)
    record.int16(0)
    record.int32(0)
    record.int32(0)
    record.int32(36000)
    record.int32(36000)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.char("N")
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.char(0)
    record.char(0)
    record.int16(1)
    record.int32(0)
    record.int32(0)
    record.char("Z")
    record.char(0)
    record.char(0)
    record.char(0)
    return record.finish()


def user_keys_record(username: str) -> bytes:
    record = AmigaRecord(56)
    record.text(username.upper(), 31)
    record.int32(1)
    record.char(0)
    record.int16(-1)
    record.int16(-1)
    record.uint16(0)
    record.uint16(38400)
    record.int32(0)
    record.int32(0)
    record.int16(0)
    return record.finish()


def user_misc_record(username: str, password: str) -> bytes:
    salt = b"nextexp!"
    password_hash = hashlib.pbkdf2_hmac(
        "sha256",
        password.encode("latin-1", "replace"),
        salt,
        100,
        dklen=32,
    )

    record = AmigaRecord(248)
    record.text(username[:9].lower(), 10)
    record.text("Sysop", 26)
    record.bytes(b"\x00" * 8, 8)
    record.bytes(b"\x00" * 8, 8)
    record.text("sysop@example.local", 50)
    record.int32(0)
    record.bytes(password_hash, 32)
    record.bytes(salt, 8)
    record.char(PWD_PBKDF2_100)
    record.char(0)
    record.char(0)
    record.char(0)
    record.int32(0)
    record.int32(0)
    record.int32(0)
    record.bytes(b"\x00" * 86, 86)
    return record.finish()


def database_exists(paths: list[str]) -> bool:
    return any(os.path.exists(path) and os.path.getsize(path) > 0 for path in paths)


def write_file(path: str, data: bytes) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as handle:
        handle.write(data)


def main() -> int:
    parser = argparse.ArgumentParser(description="Seed a sysop account into AmiExpress user files.")
    parser.add_argument("bbs_dir")
    parser.add_argument("--username", default="sysop")
    parser.add_argument("--password", default="sysop")
    parser.add_argument("--reset", action="store_true")
    args = parser.parse_args()

    paths = [
        os.path.join(args.bbs_dir, "user.data"),
        os.path.join(args.bbs_dir, "user.keys"),
        os.path.join(args.bbs_dir, "user.misc"),
    ]

    if database_exists(paths) and not args.reset:
        print("seed_sysop: existing user database found; leaving it unchanged", file=sys.stderr)
        return 0

    write_file(paths[0], user_record(args.username))
    write_file(paths[1], user_keys_record(args.username))
    write_file(paths[2], user_misc_record(args.username, args.password))
    print(f"seed_sysop: seeded {args.username}/{args.password} in slot 1")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
