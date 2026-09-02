ARG BASE_IMAGE=debian:12-slim
FROM ${BASE_IMAGE}
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl openssl python3 python3-yaml \
    && rm -rf /var/lib/apt/lists/*
COPY tests/acceptance/verify.sh /usr/local/lib/sbctl-acceptance/verify.sh
RUN chmod 0755 /usr/local/lib/sbctl-acceptance/verify.sh
ENTRYPOINT ["/usr/local/lib/sbctl-acceptance/verify.sh"]
