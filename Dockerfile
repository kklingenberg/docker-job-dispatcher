FROM busybox AS download
ARG REPO=https://github.com/kklingenberg/docker-job-dispatcher
ARG VERSION
RUN test -n "${VERSION}" && \
    wget "${REPO}/releases/download/${VERSION}/docker-job-dispatcher" -O /docker-job-dispatcher && \
    chmod +x /docker-job-dispatcher

FROM scratch
COPY --from=download /docker-job-dispatcher /usr/bin/docker-job-dispatcher
ENTRYPOINT ["/usr/bin/docker-job-dispatcher"]
