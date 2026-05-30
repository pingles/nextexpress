# syntax=docker/dockerfile:1

FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG AMIEXPRESS_LHA_URL=https://aminet.net/comm/amiex/Amix560.lha

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        fs-uae \
        libarchive-tools \
        lhasa \
        netcat-openbsd \
        python3-minimal \
        tini \
        unzip \
        xauth \
        xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/amiexpress/source \
    && curl -fsSL "${AMIEXPRESS_LHA_URL}" -o /opt/amiexpress/source/AmiExpress.lha

COPY --chmod=0755 docker/amiexpress-fsuae/entrypoint.sh /usr/local/bin/amiexpress-fsuae
COPY docker/amiexpress-fsuae/seed_sysop.py /usr/local/lib/amiexpress/seed_sysop.py
COPY docker/amiexpress-fsuae/README.md /opt/amiexpress/README.md

EXPOSE 6023

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/amiexpress-fsuae"]
