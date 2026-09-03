ARG BASE_IMAGE=debian:12-slim
FROM ${BASE_IMAGE}
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates curl openssl iproute2 python3 python3-yaml systemd systemd-sysv dbus \
    && rm -rf /var/lib/apt/lists/*
COPY tests/acceptance/verify.sh /usr/local/lib/sbctl-acceptance/verify.sh
COPY tests/acceptance/verify-real.sh /usr/local/lib/sbctl-acceptance/verify-real.sh
COPY tests/acceptance/verify-bootstrap.sh /usr/local/lib/sbctl-acceptance/verify-bootstrap.sh
COPY scripts/install.sh /usr/local/lib/sbctl-acceptance/install.sh
RUN chmod 0755 /usr/local/lib/sbctl-acceptance/verify.sh
RUN chmod 0755 /usr/local/lib/sbctl-acceptance/verify-real.sh \
    /usr/local/lib/sbctl-acceptance/verify-bootstrap.sh \
    /usr/local/lib/sbctl-acceptance/install.sh \
    && ln -sf /usr/local/lib/sbctl-acceptance/verify-real.sh /usr/local/bin/sbctl-acceptance-real

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
