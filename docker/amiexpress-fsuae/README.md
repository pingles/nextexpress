# AmiExpress FS-UAE Docker Harness

This image installs FS-UAE and downloads the official AmiExpress 5.6.0 LHA from Aminet at build time. By default, runtime bootstraps a downloadable AROS m68k system if no Kickstart/Workbench assets are mounted.

AROS is an open-source AmigaOS-compatible system. For highest compatibility with the original AmiExpress binaries, you can still provide licensed AmigaOS assets:

- a Kickstart ROM mounted at `/roms/kick.rom`, or `KICKSTART_FILE` pointing elsewhere
- a bootable Workbench directory hard drive mounted at `/amiga/workbench`
- a persistent AmiExpress BBS directory mounted at `/amiga/bbs`

Kickstart and Workbench are not redistributable, so they are deliberately not baked into the image.

## Build

```sh
docker build -f docker/amiexpress-fsuae/Dockerfile -t nextexpress/amiexpress-fsuae .
```

To use a different release archive:

```sh
docker build \
  -f docker/amiexpress-fsuae/Dockerfile \
  --build-arg AMIEXPRESS_LHA_URL=https://aminet.net/comm/amiex/Amix560.lha \
  -t nextexpress/amiexpress-fsuae .
```

## Run

```sh
docker run --rm -it \
  -p 127.0.0.1:6023:6023 \
  nextexpress/amiexpress-fsuae
```

Then connect from the host:

```sh
telnet 127.0.0.1 6023
```

The default seeded account is:

```text
sysop/sysop
```

That account is seeded with security level 255 and auto-rejoins conference 2,
the bundled `Amiga` conference. If an existing persistent BBS volume was seeded
before this default, run once with `-e RESET_SYSOP=1` to replace slot 1.

For a persistent setup with licensed Kickstart/Workbench assets:

```sh
docker run --rm -it \
  -p 127.0.0.1:6023:6023 \
  -e BOOTSTRAP_AROS=0 \
  -v "$PWD/.amiga/roms:/roms:ro" \
  -v "$PWD/.amiga/workbench:/amiga/workbench" \
  -v "$PWD/.amiga/bbs:/amiga/bbs" \
  nextexpress/amiexpress-fsuae
```

For a persistent AROS-backed setup without proprietary assets:

```sh
docker volume create nextexpress-aros-roms
docker volume create nextexpress-aros-system
docker volume create nextexpress-bbs
docker run --rm -it \
  -p 127.0.0.1:6023:6023 \
  -v nextexpress-aros-roms:/opt/aros \
  -v nextexpress-aros-system:/amiga/workbench \
  -v nextexpress-bbs:/amiga/bbs \
  nextexpress/amiexpress-fsuae
```

On first boot the entrypoint extracts AmiExpress into `/amiga/bbs`, copies the bundled support tools and libraries into the Workbench/AROS directory, writes a one-node telnet-enabled AmiExpress JSON configuration, seeds `sysop/sysop`, imports that configuration with `jsonImport` inside AmigaOS/AROS, and starts `BBS:acp` from `S:User-Startup`.

## Useful Environment Variables

- `TELNET_PORT`: telnet port written into the AmiExpress ACP config. Defaults to `6023`.
- `KICKSTART_FILE`: Kickstart ROM path inside the container. Defaults to `/roms/kick.rom`.
- `KICKSTART_EXT_FILE`: optional extended Kickstart ROM path. Used automatically for AROS.
- `WORKBENCH_DIR`: bootable Workbench directory hard drive. Defaults to `/amiga/workbench`.
- `BBS_DIR`: persistent AmiExpress directory hard drive. Defaults to `/amiga/bbs`.
- `AMIEXPRESS_ARCHIVE`: use a mounted LHA archive instead of the build-time download.
- `AMIEXPRESS_DOWNLOAD_URL`: download an archive at container startup instead of using the build-time archive.
- `BOOTSTRAP_AROS`: `auto` downloads/extracts AROS m68k when Kickstart/Workbench are missing. Use `0` to require mounted licensed assets.
- `AROS_BOOT_ISO_URL`: use a specific AROS amiga-m68k boot ISO ZIP URL instead of resolving the latest nightly.
- `SEED_SYSOP=0`: skip seeding the `sysop/sysop` account.
- `SYSOP_USERNAME` / `SYSOP_PASSWORD`: override the seeded account credentials.
- `RESET_SYSOP=1`: overwrite an existing user database slot 1 with the seeded account.
- `AUTO_CONFIGURE=0`: skip writing/importing the Docker JSON config.
- `AUTO_START=0`: skip modifying `S:User-Startup`.
- `INSTALL_TO_WORKBENCH=0`: skip copying archive tools and `aedoor.library` into Workbench.
- `DRY_RUN=1`: prepare files and the FS-UAE config, then exit before starting the emulator.

## Notes

The repo's `AmiExpress/deployment/binaries.lha` is useful as deployment input, but the Docker harness uses the Aminet release because that archive contains the compiled `acp` and `express` binaries needed to start the real telnet listener.
