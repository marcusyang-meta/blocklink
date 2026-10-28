FROM ubuntu:22.04
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 libdbus-1-3 zlib1g libstdc++6 libfreetype6 fontconfig \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 10001 blocklink && useradd -u 10001 -g 10001 -d /data -s /usr/sbin/nologin blocklink
COPY --chmod=755 blocklink-service /usr/local/bin/blocklink-service
USER 10001:10001
ENV HOME=/data
WORKDIR /data
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/blocklink-service"]
CMD ["--root", "/data"]
